#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use anyhow::Result;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn main() -> Result<()> {
    snapshort_desktop::desktop_main()
}

// Mobile/web enter through android_main/wasm_start in the lib target.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}
