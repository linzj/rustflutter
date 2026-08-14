// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/shell/common/run_configuration.h"

#include <utility>

#include "flutter/assets/directory_asset_bundle.h"
#include "flutter/common/graphics/persistent_cache.h"
#include "flutter/fml/file.h"
#include "flutter/fml/unique_fd.h"

namespace flutter {

RunConfiguration RunConfiguration::InferFromSettings(
    const Settings& settings,
    const fml::RefPtr<fml::TaskRunner>& io_worker) {
  auto asset_manager = std::make_shared<AssetManager>();

  if (fml::UniqueFD::traits_type::IsValid(settings.assets_dir)) {
    asset_manager->PushBack(std::make_unique<DirectoryAssetBundle>(
        fml::Duplicate(settings.assets_dir), true));
  }

  asset_manager->PushBack(std::make_unique<DirectoryAssetBundle>(
      fml::OpenDirectory(settings.assets_path.c_str(), false,
                         fml::FilePermission::kRead),
      true));

  return RunConfiguration(asset_manager);
}

RunConfiguration::RunConfiguration()
    : RunConfiguration(std::make_shared<AssetManager>()) {}

RunConfiguration::RunConfiguration(std::shared_ptr<AssetManager> asset_manager)
    : asset_manager_(std::move(asset_manager)) {
#if !SLIMPELLER
  PersistentCache::SetAssetManager(asset_manager_);
#endif  //  !SLIMPELLER
}

RunConfiguration::RunConfiguration(RunConfiguration&&) = default;

RunConfiguration::~RunConfiguration() = default;

bool RunConfiguration::IsValid() const {
  return asset_manager_ != nullptr;
}

bool RunConfiguration::AddAssetResolver(
    std::unique_ptr<AssetResolver> resolver) {
  if (!resolver || !resolver->IsValid()) {
    return false;
  }

  asset_manager_->PushBack(std::move(resolver));
  return true;
}

void RunConfiguration::SetEntrypoint(std::string entrypoint) {
  entrypoint_ = std::move(entrypoint);
}

void RunConfiguration::SetEntrypointArgs(
    std::vector<std::string> entrypoint_args) {
  entrypoint_args_ = std::move(entrypoint_args);
}

std::shared_ptr<AssetManager> RunConfiguration::GetAssetManager() const {
  return asset_manager_;
}

const std::string& RunConfiguration::GetEntrypoint() const {
  return entrypoint_;
}

const std::vector<std::string>& RunConfiguration::GetEntrypointArgs() const {
  return entrypoint_args_;
}

void RunConfiguration::SetEngineId(std::optional<int64_t> engine_id) {
  engine_id_ = engine_id;
}

std::optional<int64_t> RunConfiguration::GetEngineId() const {
  return engine_id_;
}

}  // namespace flutter
