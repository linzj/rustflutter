// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_SHELL_COMMON_RUN_CONFIGURATION_H_
#define FLUTTER_SHELL_COMMON_RUN_CONFIGURATION_H_

#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "flutter/assets/asset_manager.h"
#include "flutter/assets/asset_resolver.h"
#include "flutter/common/settings.h"
#include "flutter/fml/macros.h"
#include "flutter/fml/mapping.h"
#include "flutter/fml/task_runner.h"
#include "flutter/fml/unique_fd.h"

namespace flutter {

//------------------------------------------------------------------------------
/// Specifies what to run and where to find its assets.
///
/// Upstream this wrapped an IsolateConfiguration -- kernel snapshots, AOT
/// instructions, the app.dill to load. The Rust framework is statically linked
/// into the binary, so there is nothing to load: what remains is the asset
/// manager and the entry point name.
///
/// A configuration is always valid here, since there is no snapshot that could
/// be missing. IsValid() is kept so embedders that check it still compile.
class RunConfiguration {
 public:
  //----------------------------------------------------------------------------
  /// Builds a configuration from the settings the embedder supplied,
  /// registering the asset directory if one was given.
  static RunConfiguration InferFromSettings(
      const Settings& settings,
      const fml::RefPtr<fml::TaskRunner>& io_worker = nullptr);

  RunConfiguration();

  explicit RunConfiguration(std::shared_ptr<AssetManager> asset_manager);

  RunConfiguration(RunConfiguration&& config);

  ~RunConfiguration();

  bool IsValid() const;

  bool AddAssetResolver(std::unique_ptr<AssetResolver> resolver);

  void SetEntrypoint(std::string entrypoint);

  void SetEntrypointArgs(std::vector<std::string> entrypoint_args);

  std::shared_ptr<AssetManager> GetAssetManager() const;

  const std::string& GetEntrypoint() const;

  const std::vector<std::string>& GetEntrypointArgs() const;

  void SetEngineId(std::optional<int64_t> engine_id);

  std::optional<int64_t> GetEngineId() const;

 private:
  std::shared_ptr<AssetManager> asset_manager_;
  std::string entrypoint_ = "main";
  std::vector<std::string> entrypoint_args_;
  std::optional<int64_t> engine_id_;

  FML_DISALLOW_COPY_AND_ASSIGN(RunConfiguration);
};

}  // namespace flutter

#endif  // FLUTTER_SHELL_COMMON_RUN_CONFIGURATION_H_
