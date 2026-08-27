// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_FFI_RUSTFLUTTER_EXPORT_H_
#define FLUTTER_RUST_FFI_RUSTFLUTTER_EXPORT_H_

// What the engine offers the world outside its own binary: `rf_*` in
// rustflutter_ffi.h, `rf_host_run` in the host, and the two registration
// functions in rust_app_api.h. Everything else the engine contains -- Skia,
// Impeller, txt, fml, the shell -- stays inside it.
//
// The build produces the engine twice, from one compilation:
//
//     rustflutter_engine.lib      every object, folded into one archive
//     rustflutter_engine.dll      the same objects, linked
//
// and an application picks. Which is why this is annotated on the
// implementation side only, with nothing on the consumer's: a Windows caller
// reaches an exported function through the import library's thunk without
// needing `dllimport` -- that keyword matters for imported *data*, and there
// is none here -- and an ELF caller resolves it through the PLT like any other
// undefined symbol. So a consumer's declaration is the same declaration either
// way, and the header it reads does not have to know which engine it will get.
//
// RUSTFLUTTER_ENGINE_IMPLEMENTATION is set on the three targets that define
// these symbols -- rust/ffi, rust/host and runtime -- and on nothing else. It
// is not conditional on which artifact is being built, because both are built
// from the same objects. An executable that links the archive therefore also
// exports them, which costs it an export table and, on Windows, a stray .lib
// and .exp beside the binary; the alternative is compiling those three targets
// twice to say the same thing.
#if defined(RUSTFLUTTER_ENGINE_IMPLEMENTATION)
#if defined(_WIN32)
#define RF_EXPORT __declspec(dllexport)
#else
// Overrides the -fvisibility=hidden every target in this tree is compiled with;
// see //build/config/gcc:symbol_visibility_hidden. Without this the symbol is
// hidden at compile time and no version script can bring it back.
#define RF_EXPORT __attribute__((visibility("default")))
#endif
#else
#define RF_EXPORT
#endif

#endif  // FLUTTER_RUST_FFI_RUSTFLUTTER_EXPORT_H_
