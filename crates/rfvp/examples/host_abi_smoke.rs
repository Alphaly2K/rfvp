//! Minimal Rust-side smoke test for the versioned host ABI.
//!
//! Usage:
//! `cargo run -p rfvp --example host_abi_smoke --features host-runtime -- <game-root> [nls]`

use std::env;
use std::mem::size_of;

use rfvp::host_abi::runtime::{
    rfvp_frame_get_commands, rfvp_frame_get_hit_proxies, rfvp_frame_get_size,
    rfvp_frame_get_textures, rfvp_frame_release, rfvp_resources_create, rfvp_resources_destroy,
    rfvp_resources_mount_directory, rfvp_runtime_acquire_frame, rfvp_runtime_create,
    rfvp_runtime_destroy, rfvp_runtime_is_exit_requested, rfvp_runtime_step,
};
use rfvp::host_abi::v1::{
    RfvpDrawCommandV1, RfvpHitProxyV1, RfvpResourcesConfigV1, RfvpRuntimeConfigV1,
    RfvpTextureCommandV1, RFVP_STATUS_NO_FRAME, RFVP_STATUS_OK,
};

fn main() {
    let mut args = env::args().skip(1);
    let game_root = args.next().expect("game root argument");
    let nls = args
        .next()
        .map(|value| value.parse::<u32>().expect("numeric NLS value"))
        .unwrap_or(1);

    let path = game_root.as_bytes();
    let mut resources = 0u64;
    let resources_config = RfvpResourcesConfigV1 {
        struct_size: size_of::<RfvpResourcesConfigV1>() as u32,
        flags: 0,
        nls,
        reserved0: 0,
        save_root_utf8: std::ptr::null(),
        save_root_len: 0,
        reserved: [0; 4],
    };
    let status = unsafe { rfvp_resources_create(&resources_config, &mut resources) };
    assert_eq!(status, RFVP_STATUS_OK);
    let status = unsafe { rfvp_resources_mount_directory(resources, path.as_ptr(), path.len()) };
    assert_eq!(status, RFVP_STATUS_OK);

    let runtime_config = RfvpRuntimeConfigV1 {
        struct_size: size_of::<RfvpRuntimeConfigV1>() as u32,
        flags: 0,
        resources,
        requested_width: 0,
        requested_height: 0,
        reserved: [0; 4],
    };
    let mut runtime = 0u64;
    let status = unsafe { rfvp_runtime_create(&runtime_config, &mut runtime) };
    assert_eq!(status, RFVP_STATUS_OK);

    for frame_index in 0..10 {
        let status = unsafe { rfvp_runtime_step(runtime, 16) };
        assert_eq!(status, RFVP_STATUS_OK);
        if unsafe { rfvp_runtime_is_exit_requested(runtime) } != 0 {
            println!("frame {frame_index}: exit requested");
            break;
        }

        let mut frame = 0u64;
        let status = unsafe { rfvp_runtime_acquire_frame(runtime, &mut frame) };
        if status == RFVP_STATUS_NO_FRAME {
            println!("frame {frame_index}: no frame");
            continue;
        }
        assert_eq!(status, RFVP_STATUS_OK);

        let mut width = 0u32;
        let mut height = 0u32;
        let mut commands: *const RfvpDrawCommandV1 = std::ptr::null();
        let mut command_count = 0usize;
        let mut textures: *const RfvpTextureCommandV1 = std::ptr::null();
        let mut texture_count = 0usize;
        let mut proxies: *const RfvpHitProxyV1 = std::ptr::null();
        let mut proxy_count = 0usize;
        assert_eq!(
            unsafe { rfvp_frame_get_size(frame, &mut width, &mut height) },
            RFVP_STATUS_OK
        );
        assert_eq!(
            unsafe { rfvp_frame_get_commands(frame, &mut commands, &mut command_count) },
            RFVP_STATUS_OK
        );
        assert_eq!(
            unsafe { rfvp_frame_get_textures(frame, &mut textures, &mut texture_count) },
            RFVP_STATUS_OK
        );
        assert_eq!(
            unsafe { rfvp_frame_get_hit_proxies(frame, &mut proxies, &mut proxy_count) },
            RFVP_STATUS_OK
        );
        println!(
            "frame {frame_index}: {width}x{height} commands={command_count} textures={texture_count} proxies={proxy_count}"
        );
        unsafe { rfvp_frame_release(frame) };
    }

    unsafe {
        rfvp_runtime_destroy(runtime);
        rfvp_resources_destroy(resources);
    }
}
