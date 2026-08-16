// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// The drawing half of the engine C ABI: paths, gradients, transforms, clips,
// images, and the compositor's layer stack.
//
// Upstream all of this is dart:ui -- Path, Gradient, Canvas.transform,
// Canvas.clipPath, Image, SceneBuilder.push*. The engine objects underneath are
// the same ones; what changes is only that the arguments arrive as C scalars
// rather than through tonic.

#include "flutter/rust/ffi/rustflutter_ffi.h"

#include "flutter/rust/ffi/rustflutter_ffi_handles.h"

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <vector>

#include "flutter/display_list/dl_blend_mode.h"
#include "flutter/display_list/dl_color.h"
#include "flutter/display_list/effects/dl_color_source.h"
#include "flutter/display_list/effects/dl_image_filter.h"
#include "flutter/display_list/effects/dl_mask_filter.h"
#include "flutter/display_list/image/dl_image_skia.h"
#include "flutter/flow/layers/backdrop_filter_layer.h"
#include "flutter/flow/layers/clip_path_layer.h"
#include "flutter/flow/layers/clip_rect_layer.h"
#include "flutter/flow/layers/clip_rrect_layer.h"
#include "flutter/flow/layers/image_filter_layer.h"
#include "flutter/flow/layers/opacity_layer.h"
#include "flutter/flow/layers/transform_layer.h"
#include "flutter/fml/logging.h"
#include "flutter/fml/mapping.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "impeller/core/allocator.h"
#include "impeller/core/formats.h"
#include "impeller/core/texture_descriptor.h"
#include "impeller/display_list/dl_image_impeller.h"
#include "impeller/renderer/context.h"
#include "third_party/skia/include/codec/SkBmpDecoder.h"
#include "third_party/skia/include/codec/SkCodec.h"
#include "third_party/skia/include/codec/SkGifDecoder.h"
#include "third_party/skia/include/codec/SkIcoDecoder.h"
#include "third_party/skia/include/codec/SkJpegDecoder.h"
#include "third_party/skia/include/codec/SkPngDecoder.h"
#include "third_party/skia/include/codec/SkWbmpDecoder.h"
#include "third_party/skia/include/codec/SkWebpDecoder.h"
#include "third_party/skia/include/core/SkData.h"
#include "third_party/skia/include/core/SkImage.h"

namespace {

flutter::DlRect Rect(float left, float top, float right, float bottom) {
  return flutter::DlRect::MakeLTRB(left, top, right, bottom);
}

flutter::DlRoundRect RoundRect(float left,
                               float top,
                               float right,
                               float bottom,
                               float radius_x,
                               float radius_y) {
  return flutter::DlRoundRect::MakeRectXY(Rect(left, top, right, bottom),
                                          radius_x, radius_y);
}

flutter::DlTileMode ToTileMode(int32_t tile_mode) {
  switch (tile_mode) {
    case 1:
      return flutter::DlTileMode::kRepeat;
    case 2:
      return flutter::DlTileMode::kMirror;
    case 3:
      return flutter::DlTileMode::kDecal;
    default:
      return flutter::DlTileMode::kClamp;
  }
}

flutter::Clip ToClipBehavior(int32_t clip_behavior) {
  switch (clip_behavior) {
    case 0:
      return flutter::Clip::kNone;
    case 1:
      return flutter::Clip::kHardEdge;
    case 3:
      return flutter::Clip::kAntiAliasWithSaveLayer;
    default:
      return flutter::Clip::kAntiAlias;
  }
}

flutter::DlClipOp ToClipOp(int32_t clip_op) {
  return clip_op == 1 ? flutter::DlClipOp::kDifference
                      : flutter::DlClipOp::kIntersect;
}

// Turns the caller's packed ARGB array into what DlColorSource wants, and
// synthesises evenly spaced stops when none were supplied. Returns false if the
// arguments could not describe a gradient.
bool GatherStops(const uint32_t* colors,
                 const float* stops,
                 int32_t stop_count,
                 std::vector<flutter::DlColor>* out_colors,
                 std::vector<float>* out_stops) {
  if (colors == nullptr || stop_count < 2) {
    return false;
  }
  out_colors->reserve(stop_count);
  out_stops->reserve(stop_count);
  for (int32_t i = 0; i < stop_count; ++i) {
    out_colors->push_back(flutter::DlColor(colors[i]));
    out_stops->push_back(stops != nullptr
                             ? stops[i]
                             : static_cast<float>(i) / (stop_count - 1));
  }
  return true;
}

// This build defines SK_DISABLE_LEGACY_INIT_DECODERS, so nothing can be
// decoded until the codecs are registered. The shell does this during startup,
// but rf_image_decode has to work without a shell -- headless rendering and the
// unit tests both call it directly.
void EnsureCodecsRegistered() {
  static std::once_flag once;
  std::call_once(once, [] {
    SkCodecs::Register(SkPngDecoder::Decoder());
    SkCodecs::Register(SkJpegDecoder::Decoder());
    SkCodecs::Register(SkWebpDecoder::Decoder());
    SkCodecs::Register(SkGifDecoder::Decoder());
    SkCodecs::Register(SkBmpDecoder::Decoder());
    SkCodecs::Register(SkWbmpDecoder::Decoder());
    SkCodecs::Register(SkIcoDecoder::Decoder());
  });
}

/// Totals what uploading costs, and which thread paid, when anyone is asking.
///
/// Reported under RUSTFLUTTER_FRAME_STATS, next to the raster thread's own
/// numbers, so the two can be read together. `ahead` says the upload happened
/// on the IO thread before anything drew the image, which is the case that
/// costs the rasterizer nothing.
void ReportUpload(std::chrono::steady_clock::time_point started,
                  size_t bytes,
                  bool ahead) {
  static const bool enabled = std::getenv("RUSTFLUTTER_FRAME_STATS") != nullptr;
  if (!enabled) {
    return;
  }
  static std::mutex mutex;
  static int count = 0;
  static int on_raster = 0;
  static double total_ms = 0.0;
  const double ms = std::chrono::duration<double, std::milli>(
                        std::chrono::steady_clock::now() - started)
                        .count();
  std::lock_guard<std::mutex> lock(mutex);
  count++;
  total_ms += ms;
  if (!ahead) {
    on_raster++;
  }
  FML_LOG(IMPORTANT) << "texture upload: " << ms << " ms for " << (bytes / 1024)
                     << " KiB " << (ahead ? "ahead on io" : "ON RASTER")
                     << " (" << count << " images, " << total_ms << " ms, "
                     << on_raster << " on the raster thread).";
}

/// Where an image's pixels are uploaded, if the host has told us.
///
/// Upstream never uploads on the raster thread: `ImageDecoder` hands the work
/// to the IO thread's shared context so the rasterizer is never waiting on a
/// memcpy. Impeller's GLES backend is specifically why that indirection exists
/// -- `image_decoder_impeller.cc` posts to the IO runner with the comment "The
/// I/O image uploads are not threadsafe on GLES".
///
/// Set once during shell startup, from the IO thread, and read from the UI
/// thread afterwards. Nothing sets it in a headless render or a unit test, and
/// the upload then happens where it used to.
struct UploadTarget {
  fml::RefPtr<fml::TaskRunner> runner;
  std::shared_ptr<impeller::Context> context;
};

std::mutex& UploadTargetMutex() {
  static std::mutex mutex;
  return mutex;
}

UploadTarget& MutableUploadTarget() {
  static UploadTarget target;
  return target;
}

UploadTarget GetUploadTarget() {
  std::lock_guard<std::mutex> lock(UploadTargetMutex());
  return MutableUploadTarget();
}

// A decoded image that becomes a GPU texture, on the IO thread if the host gave
// us one and on the raster thread otherwise.
//
// It has to be deferred either way. Decoding happens while a widget tree is
// being built, on the UI thread, where there is no GPU context to upload to.
// `GetImpellerTexture` is the hook the dispatcher calls while rasterising; by
// then the IO thread has usually already done the work and this only hands over
// what it produced.
class RfDeferredImpellerImage final : public impeller::DlImageImpeller {
 public:
  explicit RfDeferredImpellerImage(std::shared_ptr<SkBitmap> pixels)
      : pixels_(std::move(pixels)) {}

  /// Uploads now, on the calling thread. Safe to call from the IO thread ahead
  /// of any drawing, and from the raster thread if that never happened.
  ///
  /// Returns the texture, building it at most once however many threads ask.
  std::shared_ptr<impeller::Texture> Upload(
      const std::shared_ptr<impeller::Context>& context,
      bool ahead) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (texture_ != nullptr) {
      return texture_;
    }
    if (context == nullptr || pixels_ == nullptr) {
      return nullptr;
    }
    const auto started = std::chrono::steady_clock::now();
    const SkImageInfo& info = pixels_->info();

    impeller::TextureDescriptor descriptor;
    // Host-visible rather than device-private: the private path needs a
    // command buffer and a blit pass, which is worth it for a photo being
    // decoded off-thread and not for the handful of small images baked into an
    // application.
    descriptor.storage_mode = impeller::StorageMode::kHostVisible;
    descriptor.format = impeller::PixelFormat::kR8G8B8A8UNormInt;
    descriptor.size = {info.width(), info.height()};
    descriptor.mip_count = 1;

    std::shared_ptr<impeller::Texture> texture =
        context->GetResourceAllocator()->CreateTexture(descriptor);
    if (texture == nullptr) {
      FML_LOG(ERROR) << "rf_image: could not create an Impeller texture.";
      return nullptr;
    }

    // The mapping keeps the bitmap alive for as long as the upload needs it.
    std::shared_ptr<SkBitmap> pixels = pixels_;
    auto mapping = std::make_shared<fml::NonOwnedMapping>(
        reinterpret_cast<const uint8_t*>(pixels->getAddr(0, 0)),
        descriptor.GetByteSizeOfBaseMipLevel(),
        [pixels](auto, auto) mutable { pixels.reset(); });

    if (!texture->SetContents(mapping)) {
      FML_LOG(ERROR) << "rf_image: could not upload pixels to the texture.";
      return nullptr;
    }
    ReportUpload(started, descriptor.GetByteSizeOfBaseMipLevel(), ahead);
    texture_ = std::move(texture);
    return texture_;
  }

  // |DlImageImpeller|
  std::shared_ptr<impeller::Texture> GetImpellerTexture(
      const std::shared_ptr<impeller::Context>& context) const override {
    return Upload(context, /*ahead=*/false);
  }

  // |DlImage|
  flutter::DlISize GetSize() const override {
    return pixels_ == nullptr
               ? flutter::DlISize()
               : flutter::DlISize(pixels_->width(), pixels_->height());
  }

  // |DlImage|
  bool isOpaque() const override {
    return pixels_ != nullptr && pixels_->isOpaque();
  }

  // |DlImage|
  // False: the pixels are readable from any thread, but the texture this
  // becomes is not, and callers use this to decide whether they may keep it.
  bool isUIThreadSafe() const override { return false; }

  // |DlImage|
  flutter::DlColorSpace GetColorSpace() const override {
    return flutter::DlColorSpace::kSRGB;
  }

  // |DlImage|
  size_t GetApproximateByteSize() const override {
    return sizeof(*this) + (pixels_ == nullptr ? 0 : pixels_->computeByteSize());
  }

 private:
  std::shared_ptr<SkBitmap> pixels_;
  // Written by whichever thread uploads first and read by the raster thread, so
  // the two cannot race over who allocates the texture.
  mutable std::mutex mutex_;
  mutable std::shared_ptr<impeller::Texture> texture_;

  RfDeferredImpellerImage(const RfDeferredImpellerImage&) = delete;
  RfDeferredImpellerImage& operator=(const RfDeferredImpellerImage&) = delete;
};

// The representation the active backend can actually draw.
//
// Built on first use rather than at decode time, because whether Impeller came
// up is decided by the host after an image may already have been decoded, and
// because most images are never drawn under both.
//
// First use is a recording on the UI thread, which is a frame or more before
// anything rasterises it. That gap is the whole opportunity: the upload is
// posted to the IO thread here, and by the time the raster thread asks for the
// texture it is normally already there.
const sk_sp<flutter::DlImage>& ImageFor(const RfImage* image) {
  if (!flutter::RfImpellerBackend()) {
    return image->image;
  }
  auto* mutable_image = const_cast<RfImage*>(image);
  if (mutable_image->impeller_image == nullptr && image->pixels != nullptr) {
    auto deferred = sk_make_sp<RfDeferredImpellerImage>(image->pixels);
    mutable_image->impeller_image = deferred;

    UploadTarget target = GetUploadTarget();
    if (target.runner && target.context != nullptr) {
      target.runner->PostTask([deferred, context = target.context]() {
        deferred->Upload(context, /*ahead=*/true);
      });
    }
  }
  return mutable_image->impeller_image;
}

void SetColorSource(RfPaint* paint,
                    const std::shared_ptr<flutter::DlColorSource>& source) {
  if (paint != nullptr && source != nullptr) {
    paint->paint.setColorSource(source);
  }
}

}  // namespace

// -- Paint --------------------------------------------------------------------

void rf_paint_set_opacity(RfPaint* paint, float opacity) {
  if (paint == nullptr) {
    return;
  }
  paint->paint.setOpacity(std::clamp(opacity, 0.0f, 1.0f));
}

void rf_paint_set_blend_mode(RfPaint* paint, int32_t blend_mode) {
  if (paint == nullptr) {
    return;
  }
  const auto last = static_cast<int32_t>(flutter::DlBlendMode::kLastMode);
  if (blend_mode < 0 || blend_mode > last) {
    return;
  }
  paint->paint.setBlendMode(static_cast<flutter::DlBlendMode>(blend_mode));
}

void rf_paint_set_stroke_cap(RfPaint* paint, int32_t cap) {
  if (paint == nullptr) {
    return;
  }
  switch (cap) {
    case 1:
      paint->paint.setStrokeCap(flutter::DlStrokeCap::kRound);
      break;
    case 2:
      paint->paint.setStrokeCap(flutter::DlStrokeCap::kSquare);
      break;
    default:
      paint->paint.setStrokeCap(flutter::DlStrokeCap::kButt);
      break;
  }
}

void rf_paint_set_stroke_join(RfPaint* paint, int32_t join) {
  if (paint == nullptr) {
    return;
  }
  switch (join) {
    case 1:
      paint->paint.setStrokeJoin(flutter::DlStrokeJoin::kRound);
      break;
    case 2:
      paint->paint.setStrokeJoin(flutter::DlStrokeJoin::kBevel);
      break;
    default:
      paint->paint.setStrokeJoin(flutter::DlStrokeJoin::kMiter);
      break;
  }
}

void rf_paint_set_blur(RfPaint* paint, float sigma) {
  if (paint == nullptr) {
    return;
  }
  if (sigma <= 0.0f) {
    paint->paint.setMaskFilter(nullptr);
    return;
  }
  paint->paint.setMaskFilter(std::make_shared<flutter::DlBlurMaskFilter>(
      flutter::DlBlurStyle::kNormal, sigma));
}

void rf_paint_clear_blur(RfPaint* paint) {
  if (paint != nullptr) {
    paint->paint.setMaskFilter(nullptr);
  }
}

void rf_paint_set_linear_gradient(RfPaint* paint,
                                  float x0,
                                  float y0,
                                  float x1,
                                  float y1,
                                  const uint32_t* colors,
                                  const float* stops,
                                  int32_t stop_count,
                                  int32_t tile_mode) {
  std::vector<flutter::DlColor> gradient_colors;
  std::vector<float> gradient_stops;
  if (!GatherStops(colors, stops, stop_count, &gradient_colors,
                   &gradient_stops)) {
    return;
  }
  SetColorSource(paint, flutter::DlColorSource::MakeLinear(
                            flutter::DlPoint(x0, y0), flutter::DlPoint(x1, y1),
                            gradient_colors.size(), gradient_colors.data(),
                            gradient_stops.data(), ToTileMode(tile_mode)));
}

void rf_paint_set_radial_gradient(RfPaint* paint,
                                  float center_x,
                                  float center_y,
                                  float radius,
                                  const uint32_t* colors,
                                  const float* stops,
                                  int32_t stop_count,
                                  int32_t tile_mode) {
  std::vector<flutter::DlColor> gradient_colors;
  std::vector<float> gradient_stops;
  if (!GatherStops(colors, stops, stop_count, &gradient_colors,
                   &gradient_stops)) {
    return;
  }
  SetColorSource(paint, flutter::DlColorSource::MakeRadial(
                            flutter::DlPoint(center_x, center_y), radius,
                            gradient_colors.size(), gradient_colors.data(),
                            gradient_stops.data(), ToTileMode(tile_mode)));
}

void rf_paint_set_sweep_gradient(RfPaint* paint,
                                 float center_x,
                                 float center_y,
                                 float start_degrees,
                                 float end_degrees,
                                 const uint32_t* colors,
                                 const float* stops,
                                 int32_t stop_count,
                                 int32_t tile_mode) {
  std::vector<flutter::DlColor> gradient_colors;
  std::vector<float> gradient_stops;
  if (!GatherStops(colors, stops, stop_count, &gradient_colors,
                   &gradient_stops)) {
    return;
  }
  SetColorSource(paint, flutter::DlColorSource::MakeSweep(
                            flutter::DlPoint(center_x, center_y), start_degrees,
                            end_degrees, gradient_colors.size(),
                            gradient_colors.data(), gradient_stops.data(),
                            ToTileMode(tile_mode)));
}

void rf_paint_clear_shader(RfPaint* paint) {
  if (paint != nullptr) {
    paint->paint.setColorSource(nullptr);
  }
}

// -- Path ---------------------------------------------------------------------

RfPath* rf_path_new() {
  return new RfPath();
}

void rf_path_free(RfPath* path) {
  delete path;
}

void rf_path_set_fill_type(RfPath* path, int32_t fill_type) {
  if (path == nullptr) {
    return;
  }
  path->builder.SetFillType(fill_type == 1 ? flutter::DlPathFillType::kOdd
                                           : flutter::DlPathFillType::kNonZero);
  path->Invalidate();
}

void rf_path_move_to(RfPath* path, float x, float y) {
  if (path == nullptr) {
    return;
  }
  path->builder.MoveTo(flutter::DlPoint(x, y));
  path->Invalidate();
}

void rf_path_line_to(RfPath* path, float x, float y) {
  if (path == nullptr) {
    return;
  }
  path->builder.LineTo(flutter::DlPoint(x, y));
  path->Invalidate();
}

void rf_path_quadratic_to(RfPath* path, float cx, float cy, float x, float y) {
  if (path == nullptr) {
    return;
  }
  path->builder.QuadraticCurveTo(flutter::DlPoint(cx, cy),
                                 flutter::DlPoint(x, y));
  path->Invalidate();
}

void rf_path_cubic_to(RfPath* path,
                      float cx1,
                      float cy1,
                      float cx2,
                      float cy2,
                      float x,
                      float y) {
  if (path == nullptr) {
    return;
  }
  path->builder.CubicCurveTo(flutter::DlPoint(cx1, cy1),
                             flutter::DlPoint(cx2, cy2),
                             flutter::DlPoint(x, y));
  path->Invalidate();
}

void rf_path_close(RfPath* path) {
  if (path == nullptr) {
    return;
  }
  path->builder.Close();
  path->Invalidate();
}

void rf_path_add_rect(RfPath* path,
                      float left,
                      float top,
                      float right,
                      float bottom) {
  if (path == nullptr) {
    return;
  }
  path->builder.AddRect(Rect(left, top, right, bottom));
  path->Invalidate();
}

void rf_path_add_oval(RfPath* path,
                      float left,
                      float top,
                      float right,
                      float bottom) {
  if (path == nullptr) {
    return;
  }
  path->builder.AddOval(Rect(left, top, right, bottom));
  path->Invalidate();
}

void rf_path_add_circle(RfPath* path, float x, float y, float radius) {
  if (path == nullptr) {
    return;
  }
  path->builder.AddCircle(flutter::DlPoint(x, y), radius);
  path->Invalidate();
}

void rf_path_add_rounded_rect(RfPath* path,
                              float left,
                              float top,
                              float right,
                              float bottom,
                              float radius_x,
                              float radius_y) {
  if (path == nullptr) {
    return;
  }
  path->builder.AddRoundRect(
      RoundRect(left, top, right, bottom, radius_x, radius_y));
  path->Invalidate();
}

// -- Canvas drawing -----------------------------------------------------------

void rf_canvas_draw_line(RfCanvas* canvas,
                         float x0,
                         float y0,
                         float x1,
                         float y1,
                         const RfPaint* paint) {
  if (canvas == nullptr || paint == nullptr) {
    return;
  }
  canvas->builder.DrawLine(flutter::DlPoint(x0, y0), flutter::DlPoint(x1, y1),
                           paint->paint);
}

void rf_canvas_draw_oval(RfCanvas* canvas,
                         float left,
                         float top,
                         float right,
                         float bottom,
                         const RfPaint* paint) {
  if (canvas == nullptr || paint == nullptr) {
    return;
  }
  canvas->builder.DrawOval(Rect(left, top, right, bottom), paint->paint);
}

void rf_canvas_draw_path(RfCanvas* canvas,
                         const RfPath* path,
                         const RfPaint* paint) {
  if (canvas == nullptr || path == nullptr || paint == nullptr) {
    return;
  }
  canvas->builder.DrawPath(const_cast<RfPath*>(path)->Path(), paint->paint);
}

void rf_canvas_draw_arc(RfCanvas* canvas,
                        float left,
                        float top,
                        float right,
                        float bottom,
                        float start_degrees,
                        float sweep_degrees,
                        int32_t use_center,
                        const RfPaint* paint) {
  if (canvas == nullptr || paint == nullptr) {
    return;
  }
  canvas->builder.DrawArc(Rect(left, top, right, bottom), start_degrees,
                          sweep_degrees, use_center != 0, paint->paint);
}

void rf_canvas_draw_image(RfCanvas* canvas,
                          const RfImage* image,
                          float x,
                          float y,
                          const RfPaint* paint) {
  if (canvas == nullptr || image == nullptr || image->image == nullptr) {
    return;
  }
  canvas->builder.DrawImage(image->image, flutter::DlPoint(x, y),
                            flutter::DlImageSampling::kLinear,
                            paint != nullptr ? &paint->paint : nullptr);
}

void rf_canvas_draw_image_rect(RfCanvas* canvas,
                               const RfImage* image,
                               float src_left,
                               float src_top,
                               float src_right,
                               float src_bottom,
                               float dst_left,
                               float dst_top,
                               float dst_right,
                               float dst_bottom,
                               const RfPaint* paint) {
  if (canvas == nullptr || image == nullptr) {
    return;
  }
  const sk_sp<flutter::DlImage>& drawable = ImageFor(image);
  if (drawable == nullptr) {
    return;
  }
  canvas->builder.DrawImageRect(
      drawable, Rect(src_left, src_top, src_right, src_bottom),
      Rect(dst_left, dst_top, dst_right, dst_bottom),
      flutter::DlImageSampling::kLinear,
      paint != nullptr ? &paint->paint : nullptr);
}

namespace flutter {

const sk_sp<flutter::DlImage>& RfImageDrawable(const RfImage* image) {
  static const sk_sp<flutter::DlImage> kNone;
  return image == nullptr ? kNone : ImageFor(image);
}

void RfSetImageUploadTarget(fml::RefPtr<fml::TaskRunner> runner,
                            std::shared_ptr<impeller::Context> context) {
  std::lock_guard<std::mutex> lock(UploadTargetMutex());
  MutableUploadTarget() = UploadTarget{std::move(runner), std::move(context)};
}

}  // namespace flutter

// -- Canvas state -------------------------------------------------------------

void rf_canvas_save(RfCanvas* canvas) {
  if (canvas != nullptr) {
    canvas->builder.Save();
  }
}

void rf_canvas_save_layer(RfCanvas* canvas,
                          const float* bounds_ltrb,
                          const RfPaint* paint) {
  if (canvas == nullptr) {
    return;
  }
  std::optional<flutter::DlRect> bounds;
  if (bounds_ltrb != nullptr) {
    bounds = Rect(bounds_ltrb[0], bounds_ltrb[1], bounds_ltrb[2],
                  bounds_ltrb[3]);
  }
  canvas->builder.SaveLayer(bounds,
                            paint != nullptr ? &paint->paint : nullptr);
}

void rf_canvas_restore(RfCanvas* canvas) {
  if (canvas != nullptr) {
    canvas->builder.Restore();
  }
}

int32_t rf_canvas_save_count(RfCanvas* canvas) {
  return canvas != nullptr ? canvas->builder.GetSaveCount() : 0;
}

void rf_canvas_restore_to_count(RfCanvas* canvas, int32_t count) {
  if (canvas != nullptr) {
    canvas->builder.RestoreToCount(count);
  }
}

void rf_canvas_translate(RfCanvas* canvas, float dx, float dy) {
  if (canvas != nullptr) {
    canvas->builder.Translate(dx, dy);
  }
}

void rf_canvas_scale(RfCanvas* canvas, float sx, float sy) {
  if (canvas != nullptr) {
    canvas->builder.Scale(sx, sy);
  }
}

void rf_canvas_rotate(RfCanvas* canvas, float degrees) {
  if (canvas != nullptr) {
    canvas->builder.Rotate(degrees);
  }
}

void rf_canvas_skew(RfCanvas* canvas, float sx, float sy) {
  if (canvas != nullptr) {
    canvas->builder.Skew(sx, sy);
  }
}

void rf_canvas_transform(RfCanvas* canvas,
                         float a,
                         float b,
                         float c,
                         float d,
                         float e,
                         float f) {
  if (canvas == nullptr) {
    return;
  }
  // Transform2DAffine takes rows: (mxx mxy mxt) then (myx myy myt).
  canvas->builder.Transform2DAffine(a, c, e, b, d, f);
}

void rf_canvas_clip_rect(RfCanvas* canvas,
                         float left,
                         float top,
                         float right,
                         float bottom,
                         int32_t clip_op,
                         int32_t anti_alias) {
  if (canvas == nullptr) {
    return;
  }
  canvas->builder.ClipRect(Rect(left, top, right, bottom), ToClipOp(clip_op),
                           anti_alias != 0);
}

void rf_canvas_clip_rounded_rect(RfCanvas* canvas,
                                 float left,
                                 float top,
                                 float right,
                                 float bottom,
                                 float radius_x,
                                 float radius_y,
                                 int32_t clip_op,
                                 int32_t anti_alias) {
  if (canvas == nullptr) {
    return;
  }
  canvas->builder.ClipRoundRect(
      RoundRect(left, top, right, bottom, radius_x, radius_y),
      ToClipOp(clip_op), anti_alias != 0);
}

void rf_canvas_clip_path(RfCanvas* canvas,
                         const RfPath* path,
                         int32_t clip_op,
                         int32_t anti_alias) {
  if (canvas == nullptr || path == nullptr) {
    return;
  }
  canvas->builder.ClipPath(const_cast<RfPath*>(path)->Path(),
                           ToClipOp(clip_op), anti_alias != 0);
}

// -- Layer stack --------------------------------------------------------------

void rf_layer_tree_push_transform(RfLayerTree* tree,
                                  float a,
                                  float b,
                                  float c,
                                  float d,
                                  float e,
                                  float f) {
  if (tree == nullptr) {
    return;
  }
  // DlMatrix is column-major: columns are (a, b), (c, d), then the translation.
  flutter::DlMatrix matrix(a, b, 0.0f, 0.0f,   //
                           c, d, 0.0f, 0.0f,   //
                           0.0f, 0.0f, 1.0f, 0.0f,  //
                           e, f, 0.0f, 1.0f);
  tree->Push(std::make_shared<flutter::TransformLayer>(matrix));
}

void rf_layer_tree_push_offset(RfLayerTree* tree, float dx, float dy) {
  rf_layer_tree_push_transform(tree, 1.0f, 0.0f, 0.0f, 1.0f, dx, dy);
}

void rf_layer_tree_push_clip_rect(RfLayerTree* tree,
                                  float left,
                                  float top,
                                  float right,
                                  float bottom,
                                  int32_t clip_behavior) {
  if (tree == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::ClipRectLayer>(
      Rect(left, top, right, bottom), ToClipBehavior(clip_behavior)));
}

void rf_layer_tree_push_clip_rounded_rect(RfLayerTree* tree,
                                          float left,
                                          float top,
                                          float right,
                                          float bottom,
                                          float radius_x,
                                          float radius_y,
                                          int32_t clip_behavior) {
  if (tree == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::ClipRRectLayer>(
      RoundRect(left, top, right, bottom, radius_x, radius_y),
      ToClipBehavior(clip_behavior)));
}

void rf_layer_tree_push_clip_path(RfLayerTree* tree,
                                  const RfPath* path,
                                  int32_t clip_behavior) {
  if (tree == nullptr || path == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::ClipPathLayer>(
      const_cast<RfPath*>(path)->Path(), ToClipBehavior(clip_behavior)));
}

void rf_layer_tree_push_opacity(RfLayerTree* tree,
                                uint8_t alpha,
                                float offset_x,
                                float offset_y) {
  if (tree == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::OpacityLayer>(
      alpha, flutter::DlPoint(offset_x, offset_y)));
}

void rf_layer_tree_push_backdrop_blur(RfLayerTree* tree,
                                      float sigma_x,
                                      float sigma_y) {
  if (tree == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::BackdropFilterLayer>(
      flutter::DlImageFilter::MakeBlur(sigma_x, sigma_y,
                                       flutter::DlTileMode::kClamp),
      flutter::DlBlendMode::kSrcOver, std::nullopt));
}

void rf_layer_tree_push_blur(RfLayerTree* tree, float sigma_x, float sigma_y) {
  if (tree == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::ImageFilterLayer>(
      flutter::DlImageFilter::MakeBlur(sigma_x, sigma_y,
                                       flutter::DlTileMode::kDecal)));
}

void rf_layer_tree_pop(RfLayerTree* tree) {
  if (tree != nullptr) {
    tree->Pop();
  }
}

void rf_layer_tree_push_retainable(RfLayerTree* tree) {
  if (tree == nullptr) {
    return;
  }
  tree->Push(std::make_shared<flutter::ContainerLayer>());
}

RfLayer* rf_layer_tree_pop_retained(RfLayerTree* tree) {
  if (tree == nullptr) {
    return nullptr;
  }
  auto layer = tree->PopAndTake();
  if (layer == nullptr) {
    return nullptr;
  }
  auto* handle = new RfLayer();
  handle->layer = std::move(layer);
  return handle;
}

void rf_layer_tree_add_retained(RfLayerTree* tree,
                                RfLayer* layer,
                                float dx,
                                float dy) {
  if (tree == nullptr || layer == nullptr || layer->layer == nullptr) {
    return;
  }
  // Under a transform of its own, because the retained layer knows what it
  // drew and not where it goes. A boundary that only scrolled therefore costs
  // one matrix and nothing else.
  rf_layer_tree_push_offset(tree, dx, dy);
  tree->Current().Add(layer->layer);
  tree->Pop();
}

void rf_layer_free(RfLayer* layer) {
  delete layer;
}

// -- Images -------------------------------------------------------------------

RfImage* rf_image_decode(const uint8_t* data, size_t length) {
  if (data == nullptr || length == 0) {
    return nullptr;
  }
  EnsureCodecsRegistered();
  // The bytes are copied: the caller owns them and may free them as soon as
  // this returns, whereas the decoded image outlives the call.
  sk_sp<SkData> encoded = SkData::MakeWithCopy(data, length);
  std::unique_ptr<SkCodec> codec = SkCodec::MakeFromData(std::move(encoded));
  if (codec == nullptr) {
    FML_LOG(ERROR) << "rf_image_decode: unrecognised image format.";
    return nullptr;
  }
  // Decoded straight into a bitmap of a known layout rather than into whatever
  // the file happens to use: Impeller is handed a pixel format, so guessing it
  // from the codec would mean a table of formats that mostly cannot be
  // uploaded anyway.
  auto pixels = std::make_shared<SkBitmap>();
  const SkImageInfo info = codec->getInfo()
                               .makeColorType(kRGBA_8888_SkColorType)
                               .makeAlphaType(kPremul_SkAlphaType)
                               .makeColorSpace(nullptr);
  if (!pixels->tryAllocPixels(info)) {
    FML_LOG(ERROR) << "rf_image_decode: could not allocate pixels.";
    return nullptr;
  }
  const SkCodec::Result result =
      codec->getPixels(info, pixels->getPixels(), pixels->rowBytes());
  if (result != SkCodec::kSuccess && result != SkCodec::kIncompleteInput) {
    FML_LOG(ERROR) << "rf_image_decode: decode failed with " << (int)result
                   << ".";
    return nullptr;
  }
  pixels->setImmutable();

  sk_sp<SkImage> image = pixels->asImage();
  if (image == nullptr) {
    FML_LOG(ERROR) << "rf_image_decode: could not wrap the pixels.";
    return nullptr;
  }

  auto* out = new RfImage();
  out->image = flutter::DlImageSkia::Make(std::move(image));
  out->pixels = std::move(pixels);
  return out;
}

RfImage* rf_image_from_pixels(const uint8_t* pixels,
                              int32_t width,
                              int32_t height) {
  if (pixels == nullptr || width <= 0 || height <= 0) {
    return nullptr;
  }
  // The same info rf_image_decode decodes into, for the same reason: it is what
  // the Impeller upload path expects, so choosing anything else here would only
  // move the conversion somewhere less convenient.
  const SkImageInfo info = SkImageInfo::Make(width, height, kRGBA_8888_SkColorType,
                                             kPremul_SkAlphaType, nullptr);
  auto bitmap = std::make_shared<SkBitmap>();
  if (!bitmap->tryAllocPixels(info)) {
    FML_LOG(ERROR) << "rf_image_from_pixels: could not allocate pixels.";
    return nullptr;
  }

  // Row by row rather than one memcpy: the caller's buffer is tightly packed
  // but Skia is free to give the bitmap a wider stride, and on the rows where
  // it does, a single copy would shear the image.
  const size_t row = static_cast<size_t>(width) * 4;
  auto* destination = static_cast<uint8_t*>(bitmap->getPixels());
  const size_t destination_stride = bitmap->rowBytes();
  for (int32_t y = 0; y < height; ++y) {
    std::memcpy(destination + static_cast<size_t>(y) * destination_stride,
                pixels + static_cast<size_t>(y) * row, row);
  }
  bitmap->setImmutable();

  sk_sp<SkImage> image = bitmap->asImage();
  if (image == nullptr) {
    FML_LOG(ERROR) << "rf_image_from_pixels: could not wrap the pixels.";
    return nullptr;
  }

  auto* out = new RfImage();
  out->image = flutter::DlImageSkia::Make(std::move(image));
  out->pixels = std::move(bitmap);
  return out;
}

void rf_image_free(RfImage* image) {
  delete image;
}

int32_t rf_image_width(const RfImage* image) {
  if (image == nullptr || image->image == nullptr) {
    return 0;
  }
  return image->image->GetSize().width;
}

int32_t rf_image_height(const RfImage* image) {
  if (image == nullptr || image->image == nullptr) {
    return 0;
  }
  return image->image->GetSize().height;
}
