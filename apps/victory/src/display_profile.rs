//! SDR display profiles for the supported Series X retail image.
use anyhow::{Context, Result, ensure};
use victory_loader::PreparedImage;

const RETAIL_BUILD: &str = "b99f876b74d4a77f03af5f878223b27bd647b5c35a79d5decd9f716504239e93";
const SDR_MODE: u32 = 0x0615_c100;

pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
        "480p" => Some((854, 480)),
        "720p" => Some((1280, 720)),
        "1080p" => Some((1920, 1080)),
        "1440p" => Some((2560, 1440)),
        "4k" | "2160p" => Some((3840, 2160)),
        other => {
            let (w, h) = other.split_once(['x', 'X'])?;
            let width: u32 = w.trim().parse().ok()?;
            let height: u32 = h.trim().parse().ok()?;
            if (320..=7680).contains(&width) && (240..=4320).contains(&height) {
                Some((width, height))
            } else {
                None
            }
        }
    }
}

pub fn prepare(image: &mut PreparedImage, width: u32, height: u32, render_scale: f32) -> Result<()> {
    ensure!(
        image.original_hash.to_hex().as_str() == RETAIL_BUILD,
        "Display profile scaling is unavailable for this retail game build"
    );

    const MODES: [(u32, [u8; 16]); 5] = [
        (0x0615_c0c0, [0x00, 0x05, 0, 0, 0xd0, 0x02, 0, 0, 60, 0, 0, 0, 1, 0, 0, 0]), // 720p (1280x720)
        (0x0615_c0d0, [0x80, 0x06, 0, 0, 0xa8, 0x03, 0, 0, 60, 0, 0, 0, 1, 0, 0, 0]), // 936p (1664x936)
        (0x0615_c0e0, [0x80, 0x07, 0, 0, 0x38, 0x04, 0, 0, 60, 0, 0, 0, 1, 0, 0, 0]), // 1080p (1920x1080)
        (0x0615_c0f0, [0x00, 0x0a, 0, 0, 0xa0, 0x05, 0, 0, 60, 0, 0, 0, 1, 0, 0, 0]), // 1440p (2560x1440)
        (0x0615_c100, [0x00, 0x0f, 0, 0, 0x70, 0x08, 0, 0, 60, 0, 0, 0, 1, 0, 0, 0]), // 4K (3840x2160)
    ];

    for &(rva, expected) in &MODES {
        let mode = field::<16>(image, rva)?;
        ensure!(
            *mode == expected,
            "Unexpected Series X SDR mode at {rva:#x}"
        );
        mode[..4].copy_from_slice(&width.to_le_bytes());
        mode[4..8].copy_from_slice(&height.to_le_bytes());
    }

    // Two 64 KiB-aligned, tiled RGBA8 scanout buffers. Keep the reservation at
    // stock 64 MiB so the title's UI and compositor allocations never overflow.
    let reservation = field::<5>(image, 0x0247_ad4a)?;
    ensure!(
        *reservation == [0xb9, 0, 0, 0, 4],
        "Unexpected Series X scanout reservation"
    );

    // Bypass the console title's 2 MiB scanout pool slack check (0x0247_af2b).
    // Stock is `jbe +1; int3` (0x76, 0x01, 0xcc). When rendering below 4K,
    // the 64 MiB scanout pool has more unused memory than 2 MiB; replacing with
    // NOPs allows any resolution to safely run without triggering the developer assert.
    let slack_check = field::<3>(image, 0x0247_af2b)?;
    ensure!(
        *slack_check == [0x76, 0x01, 0xcc],
        "Unexpected Series X scanout slack check"
    );
    *slack_check = [0x90, 0x90, 0x90];

    let scale = field::<4>(image, 0x028f_1e7e)?;
    ensure!(
        *scale == (2.0f32 / 3.0).to_le_bytes(),
        "Unexpected Series X temporal scene scale"
    );
    *scale = render_scale.to_le_bytes();

    // Settings uses a separate viewport setter to lay out its preview
    // panels. It loads the legacy UI dimensions, which stay at 3840×2160
    // even when the game has allocated a lower resolution UI surface. Read the
    // selected SDR mode for that viewport, before calculating its centre.
    const UI_VIEWPORT_LOAD: u32 = 0x0580_48d4;
    let viewport = field::<8>(image, UI_VIEWPORT_LOAD)?;
    ensure!(
        *viewport == [0xc5, 0xfb, 0x10, 0x0d, 0x10, 0xf0, 0x13, 0x05],
        "Unexpected Settings viewport size load"
    );
    let displacement = (SDR_MODE as i32) - (UI_VIEWPORT_LOAD as i32 + 8);
    viewport[4..].copy_from_slice(&displacement.to_le_bytes());

    let render_width = ((width as f32) * render_scale).round() as u32;
    let render_height = ((height as f32) * render_scale).round() as u32;
    tracing::info!(
        output_width = width,
        output_height = height,
        render_width,
        render_height,
        render_scale,
        "Custom display profile successfully applied to Series X executable"
    );
    println!(
        "resolution     {}x{} (render: {}x{}, scale: {:.2})",
        width,
        height,
        render_width,
        render_height,
        render_scale,
    );
    Ok(())
}

fn field<const N: usize>(image: &mut PreparedImage, rva: u32) -> Result<&mut [u8; N]> {
    let offset = image
        .info
        .rva_to_file_offset(rva)
        .context("Display profile field missing")? as usize;
    image
        .bytes
        .get_mut(offset..offset + N)
        .context("Incomplete display profile field")?
        .try_into()
        .context("Invalid display profile field length")
}
