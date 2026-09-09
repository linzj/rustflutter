# The Dart experiments the compiler's own source needs, in one place.
#
# `augmentations`: `RustBackend` and `KernelFrontend` are each one class split
# across a directory of `part` files (`lib/backend_rust/`, `lib/frontend_kernel/`).
# The sections share most of their fields, so they are not separable objects --
# `augment class` is what lets the file be split without pretending they are.
#
# It is an experiment on a dev SDK, which is a real cost: every `dart` that
# runs this compiler needs the flag, and an SDK that drops the feature stops
# the build. Written here rather than at each call site so that turning it off
# is one edit, and so that undoing the split (concatenate the parts back into
# one file, drop the `augment class` wrappers) is the only other step.
#
# Shell:  . bin/experiments.sh; dart $DART2RUST_EXPERIMENTS run ...
# Python: from paths import DART_EXPERIMENTS
# Analyzer: analysis_options.yaml names it too; there is no way to point the
#           analyzer at this file.
DART2RUST_EXPERIMENTS="--enable-experiment=augmentations"
