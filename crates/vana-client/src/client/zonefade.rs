//! A zone change fades to black, then the new zone fades in once it is ready. While the screen darkens, the old zone's
//! last frame is held: the camera has already moved to the new zone.

/// CPU scene installation precedes streamed GPU uploads. Resource acknowledgement and event startup must wait for
/// both. Actor readiness deliberately is not an input: the server can send actors after the resource acknowledgement.
pub fn resources_ready(in_scene: bool, renderer: bool, uploading: bool, downloading: bool) -> bool {
    in_scene && renderer && !uploading && !downloading
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Hold {
    /// The frame is held: the last one drawn, copied from the scene texture before the next frame draws.
    pub captured: bool,
}

/// A zone change starts (a zone line touched, or the server's 0x00B): hold the frame once a world frame was drawn and
/// the screen is not black yet.
pub fn arm(h: &mut Option<Hold>, drawn: bool, fade: f32) {
    if drawn && fade < 1.0 && h.is_none() {
        log::info!("zone fade: holding the frame");
        *h = Some(Hold::default());
    }
}

/// Each frame after the fade: arms while a zone line waits; the hold ends once the screen is black.
pub fn step(h: &mut Option<Hold>, zoning: bool, drawn: bool, fade: f32) {
    if zoning {
        arm(h, drawn, fade);
    }
    if h.is_some_and(|x| x.captured) && fade >= 1.0 {
        log::info!("zone fade: black");
        *h = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_scene_waits_for_gpu_before_resources_are_ready() {
        // Zone-in -> CPU DAT load -> renderer installed -> GPU completion. The old predicate accepted stage 2.
        let stages = [
            (false, true, false, true, false),
            (false, true, true, true, false),
            (true, true, true, true, false),
            (true, true, false, false, true),
        ];
        for (scene, renderer, uploading, downloading, expected) in stages {
            assert_eq!(resources_ready(scene, renderer, uploading, downloading), expected);
        }
        // Actor requests can still be outstanding at the last stage; they must not hold this acknowledgement.
    }

    #[test]
    fn reused_scene_needs_no_new_upload_but_waits_for_zone_handoff() {
        assert!(!resources_ready(true, true, false, true));
        assert!(resources_ready(true, true, false, false));
        // A different scene's retained renderer and a failed/no-renderer load cannot become ready.
        assert!(!resources_ready(false, true, false, false));
        assert!(!resources_ready(true, false, false, false));
        // Upload ownership remains authoritative even after the download indicator is cleared.
        assert!(!resources_ready(true, true, true, false));
    }

    #[test]
    fn holds_until_black() {
        let mut h = None;
        // Not zoning, or no world drawn yet (the login): nothing.
        step(&mut h, false, true, 0.0);
        step(&mut h, true, false, 0.0);
        assert!(h.is_none());
        step(&mut h, true, true, 0.0);
        assert!(h.is_some());
        // Not captured yet: kept even at black.
        step(&mut h, true, true, 1.0);
        assert!(h.is_some());
        h.as_mut().unwrap().captured = true;
        step(&mut h, true, true, 0.6);
        assert!(h.is_some());
        step(&mut h, true, true, 1.0);
        assert!(h.is_none());
        // Still waiting on the zone line, already black: no new hold.
        step(&mut h, true, true, 1.0);
        assert!(h.is_none());
    }
}
