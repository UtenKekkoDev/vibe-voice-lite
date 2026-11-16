# Local build-script compatibility patch

Based on the official ort-sys 2.0.0-rc.10 crate. Upstream files were verified
against the vendored crate checksums before applying this change.

Only `copy_libraries` in `build.rs` is changed: locate the Cargo profile
directory by `PROFILE` instead of assuming three ancestors from `OUT_DIR`,
and create DLL destination directories before copying. Cargo's nested
`build/<package>/<hash>/out` layout otherwise sends DLLs to the wrong directory
and fails on a fresh Windows checkout.

The ONNX Runtime bindings, download URLs, download verification and runtime
code are unchanged. Original MIT and Apache-2.0 licenses are included.
