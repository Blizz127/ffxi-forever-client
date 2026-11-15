use super::*;

#[test]
#[ignore = "requires locally owned DATs; run explicitly in the offline fixture"]
fn owned_login_actions_focus_cancel_busy_and_error() {
    let root = Config::load().ffxi_dir();
    let index = DatIndex::open(&root).expect("owned DAT fixture");
    let dll = std::fs::read(root.join("FFXiMain.dll")).expect("owned DLL fixture");
    let mut lu = GameUi::new(GameUiData::load(&index, &dll).unwrap());
    let mut values = ["Private", "fixture.invalid", "synthetic", "synthetic-password", ""].map(String::from);
    let mut fields: Vec<_> = values
        .iter_mut()
        .map(|text| LoginField {
            label: "Field",
            text,
            secret: false,
            hint: "",
            help: "",
            options: vec![],
            read_only: false,
        })
        .collect();
    let n = fields.len();
    let first = n + 1;
    let exit = first + login_actions().len() - 1;
    let mut focus = 0;
    let mut combo = None;
    let mut remember = false;
    let mut input = UiInput {
        map_shortcut: false,
        #[cfg(feature = "forever")]
        tracker_shortcut: false,
        #[cfg(feature = "forever")]
        journal_shortcut: false,
        #[cfg(feature = "forever")]
        minimal_shortcut: false,
        #[cfg(feature = "forever")]
        recipe_shortcut: false,
        #[cfg(feature = "forever")]
        recipe_key: None,
        pointer: None,
        moved: false,
        clicked: false,
        pressed: false,
        down: false,
        released: false,
        right_clicked: false,
        middle_clicked: false,
        wheel: 0,
        page: false,
        events: vec![],
    };
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let mut draw = |input: &UiInput,
                    focus: &mut usize,
                    busy,
                    status: &str,
                    fields: &mut [LoginField],
                    combo: &mut Option<(usize, usize)>| {
        let mut view = None;
        let action = login_window(
            &mut lu,
            input,
            [1280, 800],
            fields,
            false,
            combo,
            &mut remember,
            focus,
            busy,
            status,
            None,
            "test",
            &mut vec![],
            Some(&mut view),
            1.0,
        );
        (action, view.unwrap(), lu.modern_help.clone().unwrap().0)
    };
    let (_, view, _) = draw(&input, &mut focus, false, "", &mut fields, &mut combo);
    let labels: Vec<_> = view.buttons.iter().map(|b| b.1).collect();
    if cfg!(feature = "forever") {
        assert_eq!(labels, ["Log in", "Exit"]);
        assert!(view.account_guidance.unwrap().contains("Banshee Realm"));
    } else {
        assert_eq!(labels, ["Log in", "Create account", "Exit"]);
        assert!(view.account_guidance.is_none());
    }
    // Real form mouse hit testing: preserved existing-account login, then Exit.
    for (k, expected) in [(0, LoginChoice::LogIn), (view.buttons.len() - 1, LoginChoice::Exit)] {
        let r = view.buttons[k].0;
        input.pointer = Some((r[0] + 2.0, r[1] + 2.0));
        input.clicked = true;
        assert!(draw(&input, &mut focus, false, "", &mut fields, &mut combo).0 == Some(expected));
    }
    assert!(draw(&input, &mut focus, true, "Connecting", &mut fields, &mut combo).0.is_none());
    input.pointer = None;
    input.clicked = false;
    input.events = vec![key(egui::Key::Escape)];
    assert!(draw(&input, &mut focus, false, "", &mut fields, &mut combo).0.is_none());
    assert_eq!(focus, exit);
    input.events = vec![key(egui::Key::Enter)];
    assert!(draw(&input, &mut focus, false, "", &mut fields, &mut combo).0 == Some(LoginChoice::Exit));
    focus = first;
    input.events = vec![key(egui::Key::ArrowRight)];
    draw(&input, &mut focus, false, "", &mut fields, &mut combo);
    assert_eq!(focus, first + 1);
    input.events = vec![key(egui::Key::Tab)];
    focus = exit;
    draw(&input, &mut focus, false, "", &mut fields, &mut combo);
    assert_eq!(focus, exit + 1);
    input.events = vec![key(egui::Key::Enter)];
    assert!(draw(&input, &mut focus, false, "", &mut fields, &mut combo).0 == Some(LoginChoice::TwoFa));
    input.events.clear();
    focus = 0;
    assert_eq!(
        draw(&input, &mut focus, false, "Synthetic sign-in refusal", &mut fields, &mut combo).2,
        "Synthetic sign-in refusal"
    );
    // Dropdown cancel closes only dropdown, never quits or creates an account.
    fields[0].options = vec!["Private".into()];
    combo = Some((0, 0));
    input.events = vec![key(egui::Key::Escape)];
    assert!(draw(&input, &mut focus, false, "", &mut fields, &mut combo).0.is_none());
    assert!(combo.is_none());
    assert_eq!(focus, 0);
    fields[3].text.clear();
    focus = first;
    input.events = vec![key(egui::Key::Enter)];
    assert!(draw(&input, &mut focus, false, "", &mut fields, &mut combo).0.is_none());
    input.events.clear();
    focus = usize::MAX;
    draw(&input, &mut focus, false, "", &mut fields, &mut combo);
    assert_eq!(focus, exit + 1);
}
