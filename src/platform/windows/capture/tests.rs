use super::*;
use crate::core::{
    display::ColorMode,
    long_image::{Step, Stitcher, fixture},
};
use windows::{
    Win32::{
        Graphics::{Dwm::DwmFlush, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

struct TestWindow(HWND);
impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: the fixture HWND was created on this test thread.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

impl TestWindow {
    fn show(area: PixelRect) -> Result<Self> {
        let (width, height) = area.size().ok_or_else(|| fail("性能夹具尺寸无效"))?;
        // SAFETY: create a diagnostic-only popup that never activates or takes input.
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("kiki 长截图取帧性能夹具"),
                WS_POPUP,
                area.left,
                area.top,
                width as i32,
                height as i32,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            let window = Self(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
            Ok(window)
        }
    }

    fn draw(&self, image: &Image) -> Result<()> {
        let (width, height) = image
            .bounds()
            .size()
            .ok_or_else(|| fail("性能夹具像素无效"))?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: draw only into the test-owned popup with a borrowed, tightly packed BGRA buffer.
        // ReleaseDC always runs before returning, and flushing does not activate the popup.
        unsafe {
            let dc = GetDC(Some(self.0));
            if dc.is_invalid() {
                return Err(fail("无法取得性能夹具绘制上下文"));
            }
            let lines = SetDIBitsToDevice(
                dc,
                0,
                0,
                width,
                height,
                0,
                0,
                0,
                height,
                image.pixels().as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
            let _ = ReleaseDC(Some(self.0), dc);
            if lines != height as i32 {
                return Err(fail("性能夹具绘制失败"));
            }
            GdiFlush().ok()?;
            DwmFlush()
        }
    }
}

#[test]
#[ignore = "opt-in Release measurement using a visible non-activating SDR fixture"]
fn native_stream_reads_and_stitches_fast_scroll_frames() {
    const WIDTH: i32 = 420;
    const HEIGHT: i32 = 260;
    const SHIFT: i32 = 83;
    const SAMPLES: i32 = 36;

    let screen = display::enumerate()
        .unwrap()
        .into_iter()
        .map(|s| s.info)
        .find(|s| {
            s.color == ColorMode::Sdr && s.bounds.size().is_some_and(|(w, h)| w >= 640 && h >= 480)
        })
        .expect("native stream probe requires an SDR display");
    let area = PixelRect {
        left: screen.bounds.left + 35,
        top: screen.bounds.top + 80,
        right: screen.bounds.left + 35 + WIDTH,
        bottom: screen.bounds.top + 80 + HEIGHT,
    };
    let window = TestWindow::show(area).unwrap();
    window.draw(&fixture(WIDTH, 0, HEIGHT)).unwrap();
    let (painted, requests) = mpsc::sync_channel::<(i32, Instant)>(1);
    let (captured, responses) = mpsc::sync_channel::<f64>(1);

    let worker = std::thread::spawn(move || -> Result<()> {
        let cancel = AtomicBool::new(false);
        let stream = RegionStream::open(area, &screen)?;
        let mut pixels = Image::new(area).map_err(|e| fail(&e))?;
        let mut stitched = None;
        while let Ok((offset, started)) = requests.recv() {
            let expected = fixture(WIDTH, offset, HEIGHT);
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if stream.read(&mut pixels, Duration::from_millis(12), &cancel)?
                    && pixels.pixels() == expected.pixels()
                {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(fail(&format!("性能夹具未捕获预期滚动位置 {offset}")));
                }
            }
            if let Some(stitch) = &mut stitched {
                let stitch: &mut Stitcher = stitch;
                if stitch.accept(&pixels, true).map_err(|e| fail(&e))? != Step::Added {
                    return Err(fail("原生快滚夹具拼接失败"));
                }
            } else {
                stitched = Some(Stitcher::new(expected).map_err(|e| fail(&e))?);
            }
            if captured
                .send(started.elapsed().as_secs_f64() * 1000.)
                .is_err()
            {
                return Ok(());
            }
        }
        let stitched = stitched.ok_or_else(|| fail("原生快滚夹具无输出"))?;
        if stitched.image().pixels() != fixture(WIDTH, 0, HEIGHT + SHIFT * SAMPLES).pixels() {
            return Err(fail("原生快滚夹具输出像素不连续"));
        }
        Ok(())
    });
    println!("sample,offset_px,draw_to_stitch_ms");
    let measurements = (|| {
        for sample in 0..=SAMPLES {
            let offset = sample * SHIFT;
            let image = fixture(WIDTH, offset, HEIGHT);
            let started = Instant::now();
            if sample != 0 {
                window.draw(&image)?;
            }
            painted
                .send((offset, started))
                .map_err(|_| fail("取帧性能线程已退出"))?;
            let elapsed = responses
                .recv_timeout(Duration::from_secs(5))
                .map_err(|e| fail(&e.to_string()))?;
            println!("{sample},{offset},{elapsed:.3}");
        }
        Ok::<(), windows::core::Error>(())
    })();
    drop(painted);
    drop(responses);
    worker.join().unwrap().unwrap();
    measurements.unwrap();
}
