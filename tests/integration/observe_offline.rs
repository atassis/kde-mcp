use kde_mcp::tools::observe::{FocusedWindow, ObserveResponse, Screen};
use kde_mcp::trust::{Tagged, Trust};

#[test]
fn observe_response_shape_matches_spec_6_1() {
    let resp = ObserveResponse {
        mode: "live".into(),
        cursor: [100, 200],
        focused_window: Some(FocusedWindow {
            uuid: "win#abc".into(),
            title: Tagged {
                trust: Trust::External,
                text: "Document — kate".into(),
            },
            class: Tagged {
                trust: Trust::Trusted,
                text: "kate".into(),
            },
            pid: Some(1234),
            geometry: Some([0, 0, 800, 600]),
            screen: Some("DP-1".into()),
            minimized: false,
            maximized: false,
            fullscreen: false,
        }),
        focused_element: None,
        windows: Some(vec![]),
        screens: Some(vec![Screen {
            name: "DP-1".into(),
            geometry: [0, 0, 2560, 1440],
            scale: 1.5,
            primary: true,
        }]),
        warnings: vec!["test".into()],
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["mode"], "live");
    assert_eq!(v["cursor"][0], 100);
    assert_eq!(v["focused_window"]["uuid"], "win#abc");
    assert_eq!(v["focused_window"]["title"]["trust"], "external");
    assert_eq!(v["focused_window"]["class"]["trust"], "trusted");
    assert_eq!(v["screens"][0]["scale"], 1.5);
    // focused_element should serialise to absent (skip_serializing_if).
    assert!(v.get("focused_element").is_none());
}

#[test]
fn observe_response_omits_optional_fields_when_none() {
    let resp = ObserveResponse {
        mode: "virtual".into(),
        cursor: [0, 0],
        focused_window: None,
        focused_element: None,
        windows: None,
        screens: None,
        warnings: vec![],
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["mode"], "virtual");
    assert!(v.get("focused_window").is_none());
    assert!(v.get("windows").is_none());
    assert!(v.get("screens").is_none());
    assert!(v.get("warnings").is_none()); // skip_serializing_if = Vec::is_empty
}
