//! Derived diagnostics only. Capture runtime and persisted intent remain the owners.
use hear_protocol::{CaptureBlocker as B, CaptureState, ServiceState, SourcePhase as P};

fn phase(enabled: bool, active: bool, busy: bool, wanted: bool, failed: bool) -> P {
    if active {
        return if wanted { P::Recording } else { P::Stopping };
    }
    if !enabled {
        return P::Disabled;
    }
    if !wanted {
        return if busy { P::Stopping } else { P::Idle };
    }
    // A retry keeps its previous failure until the capture callback confirms frames.
    if failed { P::Unavailable } else { P::Starting }
}

#[allow(clippy::too_many_arguments)]
pub fn apply(
    s: &mut ServiceState,
    models: bool,
    cloud: bool,
    allowed: bool,
    login: bool,
    maintenance: bool,
    mic_busy: bool,
    system_busy: bool,
) {
    s.capture_blocker = if s.storage_paused {
        B::StorageLimit
    } else if !s.microphone_enabled && !s.system_enabled {
        B::SourcesDisabled
    } else if maintenance {
        B::Maintenance
    } else if !models {
        B::ModelNotReady
    } else if !cloud {
        B::SetupIncomplete
    } else if s.enabled_intent && !allowed {
        if login {
            B::SessionStopped
        } else {
            B::QueryOnly
        }
    } else {
        B::None
    };
    let wanted = s.enabled_intent && !s.paused_intent && s.capture_blocker == B::None;
    s.microphone_phase = phase(
        s.microphone_enabled,
        s.microphone_recording,
        mic_busy,
        wanted && s.microphone_enabled,
        s.microphone_reason.is_some(),
    );
    s.system_phase = phase(
        s.system_enabled,
        s.system_recording,
        system_busy,
        wanted && s.system_enabled,
        s.system_reason.is_some(),
    );
    if wanted && !s.microphone_recording && !s.system_recording {
        if s.microphone_phase == P::Unavailable || s.system_phase == P::Unavailable {
            s.capture_blocker = B::SourceUnavailable;
            s.capture_state = CaptureState::Stopped;
        } else {
            s.capture_state = CaptureState::Starting;
        }
    }
    if s.microphone_phase == P::Stopping || s.system_phase == P::Stopping {
        s.capture_state = CaptureState::Stopping;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> ServiceState {
        serde_json::from_value(serde_json::json!({"revision":1,"enabled_intent":true,"paused_intent":false,"storage_paused":false,"microphone_enabled":true,"system_enabled":true,"capture_state":"stopped","reason":null,"microphone_recording":false,"system_recording":false,"microphone_reason":null,"system_reason":null,"microphone_phase":"idle","system_phase":"idle","capture_blocker":"none"})).unwrap()
    }
    #[test]
    fn blockers_are_structured_and_stopping_tracks_real_activity() {
        let mut s = state();
        apply(&mut s, false, true, true, false, false, false, false);
        assert_eq!(s.capture_blocker, B::ModelNotReady);
        apply(&mut s, true, false, true, false, false, false, false);
        assert_eq!(s.capture_blocker, B::SetupIncomplete);
        apply(&mut s, true, true, false, false, false, false, false);
        assert_eq!(s.capture_blocker, B::QueryOnly);
        apply(&mut s, true, true, false, true, false, false, false);
        assert_eq!(s.capture_blocker, B::SessionStopped);
        apply(&mut s, true, true, true, false, true, false, false);
        assert_eq!(s.capture_blocker, B::Maintenance);
        s.microphone_recording = true;
        s.paused_intent = true;
        apply(&mut s, true, true, true, false, false, true, false);
        assert_eq!(s.microphone_phase, P::Stopping);
        assert!(matches!(s.capture_state, CaptureState::Stopping));
        s.microphone_recording = false;
        s.storage_paused = true;
        apply(&mut s, true, true, true, false, false, false, false);
        assert_eq!(s.capture_blocker, B::StorageLimit);
        assert_eq!(s.microphone_phase, P::Idle);
    }
    #[test]
    fn disabled_and_failed_sources_do_not_masquerade_as_startup() {
        let mut s = state();
        s.system_enabled = false;
        apply(&mut s, true, true, true, false, false, true, false);
        assert_eq!(s.system_phase, P::Disabled);
        assert_eq!(s.microphone_phase, P::Starting);
        assert!(matches!(s.capture_state, CaptureState::Starting));
        s.microphone_reason = Some("any diagnostic wording".into());
        apply(&mut s, true, true, true, false, false, true, false);
        assert_eq!(s.capture_blocker, B::SourceUnavailable);
        assert_eq!(s.microphone_phase, P::Unavailable);
        assert!(matches!(s.capture_state, CaptureState::Stopped));
        s.microphone_enabled = false;
        apply(&mut s, true, true, true, false, false, false, false);
        assert_eq!(s.capture_blocker, B::SourcesDisabled);
    }
    #[test]
    fn active_thread_is_recording_and_retry_failure_stays_unavailable() {
        assert_eq!(phase(true, true, true, true, false), P::Recording);
        assert_eq!(phase(true, false, true, true, true), P::Unavailable);
        assert_eq!(phase(true, false, true, true, false), P::Starting);
    }
    #[test]
    fn disabled_source_still_stopping_is_not_disabled_or_healthy() {
        assert_eq!(phase(false, true, true, false, false), P::Stopping);
        assert_eq!(phase(false, false, false, false, true), P::Disabled);
        assert_eq!(phase(true, false, false, false, true), P::Idle);
    }
}
