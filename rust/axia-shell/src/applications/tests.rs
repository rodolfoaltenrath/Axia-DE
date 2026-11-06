use super::*;

#[test]
fn mime_validation_and_buffer_contract() {
    for good in ["text/plain", "x-scheme-handler/https", "image/svg+xml"] {
        assert!(mime::valid(good.as_bytes()));
    }
    for bad in [
        "",
        "../x",
        "Text/plain",
        "text/",
        "/plain",
        "a/b/c",
        "text/pl ain",
    ] {
        assert!(!mime::valid(bad.as_bytes()));
    }
    for bad in [
        "../foo.desktop",
        ".foo.desktop",
        "-foo.desktop",
        "x/y.desktop",
        "foo",
    ] {
        assert!(!mime::desktop_valid(bad.as_bytes()));
    }
    let t = c"[Default Applications]\ntext/plain=../bad.desktop;a.desktop;longer.desktop;\n";
    unsafe {
        let mut out = [42; 12];
        assert_eq!(
            mime::mimeapps_default(t.as_ptr(), c"text/plain".as_ptr(), out.as_mut_ptr(), 10),
            0
        );
        assert_eq!(&out[10..], &[42, 42]);
        assert_eq!(CStr::from_ptr(out.as_ptr()).to_bytes(), b"a.desktop");
        assert_eq!(
            mime::mimeapps_default(t.as_ptr(), c"text/plain".as_ptr(), out.as_mut_ptr(), 9),
            -1
        );
        assert_eq!(
            mime::mimeapps_default(
                t.as_ptr(),
                c"text/plain".as_ptr(),
                std::ptr::null_mut(),
                160
            ),
            -1
        );
        let result = mime::mimeapps_set_default(
            std::ptr::null(),
            c"text/plain".as_ptr(),
            c"a.desktop".as_ptr(),
        );
        assert!(!result.is_null());
        assert!(bytes(result).starts_with(b"[Default Applications]"));
        libc::free(result.cast());
    }
}
#[test]
fn associations_keep_other_preferences_and_remove_all_target_keys() {
    let text=b"# comment\r\n[Default Applications]\r\nimage/png=photos.desktop;\r\ntext/plain=old.desktop;\n\n[Added Associations]\ntext/plain=old.desktop;new.desktop;old.desktop;\n[Removed Associations]\ntext/plain=blocked.desktop;\n";
    let changed = mime::set(text, b"text/plain", b"new.desktop");
    let preserved = b"image/png=photos.desktop;\r\n";
    assert!(changed.windows(preserved.len()).any(|s| s == preserved));
    assert_eq!(
        mime::entries(&changed, b"Default Applications", b"text/plain").collect::<Vec<_>>(),
        vec![b"new.desktop".as_slice()]
    );
    assert_eq!(
        mime::entries(&changed, b"Added Associations", b"text/plain").collect::<Vec<_>>(),
        vec![b"new.desktop".as_slice(), b"old.desktop", b"old.desktop"]
    );
    let reset = mime::unset(&changed, b"text/plain");
    assert_eq!(
        mime::entries(&reset, b"Default Applications", b"text/plain").count(),
        0
    );
    assert!(mime::removed(&reset, b"text/plain", b"blocked.desktop"));
    assert_eq!(
        mime::unset(b"# no final newline", b"text/plain"),
        b"# no final newline"
    );
}
#[test]
fn glob_weight_length_case_and_invalid_rows() {
    let globs=b"50:text/plain:*.txt\n50:text/special:*.special.txt\n80:application/x-high:*.txt\n80:text/case:*.TXT:cs\n100:invalid:*.txt\n";
    assert_eq!(
        mime::guess(globs, b"/dir/a.TXT", 128).unwrap(),
        b"text/case"
    );
    assert_eq!(
        mime::guess(globs, b"a.txt", 128).unwrap(),
        b"application/x-high"
    );
    assert_eq!(
        mime::guess(
            b"50:text/plain:*.txt\n50:text/special:*.special.txt",
            b"a.special.txt",
            128
        )
        .unwrap(),
        b"text/special"
    );
    assert!(mime::guess(globs, b"a.bin", 128).is_none());
    assert!(mime::guess(b"-1:text/plain:*.txt", b"a.txt", 128).is_none());
}
fn entry() -> desktop::Entry {
    desktop::parse(b"[Desktop Entry]\nType=Application\nName=App\nName[pt]=Aplicativo\nName[pt_BR]=Aplicativo BR\nIcon=app-icon\nExec=/usr/bin/true %f\nActions=new;new;invalid_name;unlisted;\n[Desktop Action new]\nName=New\\sWindow\nName[pt]=Nova\\sJanela\nExec=/usr/bin/true\n[Desktop Action invalid_name]\nName=Invalid\nExec=/usr/bin/false\n[Desktop Action missing]\nName=Missing\n",b"/apps/app.desktop",b"pt-BR")
}
#[test]
fn desktop_translation_and_action_allowlist() {
    let e = entry();
    assert_eq!(value(&e.app.name), b"Aplicativo BR");
    assert_eq!(value(&e.app.name_c), b"App");
    let acts = e.ordered_actions();
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0].label, b"Nova Janela");
    assert!(e.action(b"missing").is_none());
    assert!(e.action(b"invalid_name").is_none());
    assert!(e.action(b"../new").is_none());
    for id in ["lpk-notes", "../x", "-x", "x+y", "x..y"] {
        assert!(!desktop::id_valid(id.as_bytes()));
    }
    assert!(desktop::id_valid(b"org.example.App"));
    let text = b"[Desktop Entry]\nName=Default\nName[en]=English\nName[pt_BR]=Brasil\n";
    assert_eq!(
        value(&desktop::parse(text, b"app.desktop", b"en.UTF-8").app.name),
        b"English"
    );
    assert_eq!(
        value(
            &desktop::parse(text, b"app.desktop", b"pt_BR.UTF-8")
                .app
                .name
        ),
        b"Brasil"
    );
}
#[test]
fn exec_fields_never_split_caller_paths_or_use_a_shell() {
    let e = entry();
    let (args, used) = launch::split(
        b"/usr/bin/true %f %F %i %c %k %% \"\"",
        &e,
        Some(b"/home/user/a;$(touch p) file.txt"),
    )
    .unwrap();
    assert!(used);
    assert_eq!(args[1], b"/home/user/a;$(touch p) file.txt");
    assert_eq!(args[2], b"--icon");
    assert_eq!(args[3], b"app-icon");
    assert_eq!(args[4], b"Aplicativo BR");
    assert_eq!(args[5], b"/apps/app.desktop");
    assert_eq!(args[6], b"%");
    assert_eq!(args[7], b"");
    assert_eq!(args.len(), 8);
    let (args, used) = launch::split(b"app %u", &e, Some(b"/a b/#c\xff")).unwrap();
    assert!(used);
    assert_eq!(args[1], b"file:///a%20b/%23c%FF");
    assert!(launch::split(b"app \"unclosed", &e, None).is_none());
    assert!(launch::split(&vec![b'a'; 4096], &e, None).is_none());
    assert!(launch::split(format!("app {}", "x ".repeat(65)).as_bytes(), &e, None).is_none());
}
#[test]
fn sandbox_does_not_grant_other_app_configuration() {
    for id in [
        "Code",
        "SSH",
        "axia-private",
        "lordos-helper",
        "autostart",
        "systemd",
        "mozilla",
        ".hidden",
    ] {
        assert!(!launch::own_dir_ok(id.as_bytes()), "{id}");
    }
    assert!(launch::own_dir_ok(b"org.example.Editor"));
}

#[test]
fn preferences_catalog_and_launch_in_isolated_process() {
    // Environment and fsops roots belong to the desktop process, so exercise
    // those contracts in a subprocess without mutating other parallel tests.
    let root = std::env::temp_dir().join(format!("axia-applications-test-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let cleanup = Cleanup(root);
    for child in [
        "applications::tests::environment_contract_child",
        "applications::launch::tests::sandbox_plan_contract_child",
    ] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", child, "--nocapture"])
            .env("AXIA_APPLICATIONS_TEST_ROOT", &cleanup.0)
            .env("HOME", cleanup.0.join("home"))
            .env("XDG_CONFIG_HOME", cleanup.0.join("config"))
            .env("XDG_RUNTIME_DIR", cleanup.0.join("run"))
            .env_remove("LORDOS_SESSIOND")
            .status()
            .unwrap();
        assert!(result.success());
    }
}

#[test]
fn environment_contract_child() {
    let Some(root) = std::env::var_os("AXIA_APPLICATIONS_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let home = root.join("home");
    let config = root.join("config");
    let system = root.join("usr/share/applications");
    let user = home.join(".local/share/applications");
    for p in [
        &system,
        &user,
        &config,
        &root.join("usr/share/mime"),
        &root.join("etc/xdg"),
    ] {
        std::fs::create_dir_all(p).unwrap();
    }
    let fifo = system.join("pipe.desktop");
    unsafe {
        assert_eq!(
            libc::mkfifo(cstring(fifo.as_os_str().as_bytes()).as_ptr(), 0o600),
            0
        );
    }
    assert!(desktop::read(fifo.as_os_str().as_bytes(), b"en").is_none());
    let write = |p: &Path, s: &[u8]| std::fs::write(p, s).unwrap();
    unsafe {
        defaults::defaults_set_root(cstring(root.as_os_str().as_bytes()).as_ptr());
    }
    extern "C" {
        fn fsops_init(home: *const c_char, user: *const c_char);
    }
    unsafe {
        fsops_init(
            cstring(home.as_os_str().as_bytes()).as_ptr(),
            c"axia-test".as_ptr(),
        );
    }
    write(&system.join("beta.desktop"),b"[Desktop Entry]\nType=Application\nName=Beta\nExec=/usr/bin/touch %f\nMimeType=text/plain;\nActions=new;\n[Desktop Action new]\nName=New\nExec=/usr/bin/touch action-marker\n");
    write(&system.join("zeta.desktop"),b"[Desktop Entry]\nType=Application\nName=Zeta\nExec=/usr/bin/true %f\nMimeType=text/plain;\n");
    write(&system.join("hidden.desktop"),b"[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/true %f\nMimeType=text/plain;\n");
    write(
        &user.join("hidden.desktop"),
        b"[Desktop Entry]\nHidden=true\n",
    );
    write(
        &system.join("lpk-notes.desktop"),
        b"[Desktop Entry]\nType=Application\nName=Imposter\nExec=/usr/bin/true\n",
    );
    assert_eq!(desktop::list(b"en").len(), 2);
    assert_eq!(
        launch::default_for(b"text/plain").unwrap(),
        (3, b"beta.desktop".to_vec())
    );
    write(
        &system.join("mimeinfo.cache"),
        b"[MIME Cache]\ntext/plain=zeta.desktop;beta.desktop;\n",
    );
    assert_eq!(
        launch::default_for(b"text/plain").unwrap(),
        (3, b"zeta.desktop".to_vec())
    );
    write(
        &root.join("etc/xdg/mimeapps.list"),
        b"[Default Applications]\ntext/plain=missing.desktop;beta.desktop;\n",
    );
    assert_eq!(
        launch::default_for(b"text/plain").unwrap(),
        (2, b"beta.desktop".to_vec())
    );
    unsafe {
        assert_eq!(
            defaults::defaults_set(c"text/plain".as_ptr(), c"zeta.desktop".as_ptr()),
            0
        );
        assert!(config.join("mimeapps.list").exists());
        assert!(!home.join(".config/mimeapps.list").exists());
        assert_eq!(
            launch::default_for(b"text/plain").unwrap(),
            (1, b"zeta.desktop".to_vec())
        );
        assert_eq!(defaults::defaults_unset(c"text/plain".as_ptr()), 0);
        assert_eq!(
            launch::default_for(b"text/plain").unwrap(),
            (2, b"beta.desktop".to_vec())
        );
        assert_eq!(defaults::defaults_set_terminal(c"foot.desktop".as_ptr()), 0);
        let mut terminal = [0; 160];
        assert_eq!(defaults::defaults_terminal(terminal.as_mut_ptr(), 160), 0);
        assert_eq!(value(&terminal), b"foot.desktop");
    }
    write(
        &config.join("mimeapps.list"),
        b"[Removed Associations]\ntext/plain=beta.desktop;\n",
    );
    assert_eq!(
        launch::default_for(b"text/plain").unwrap(),
        (3, b"zeta.desktop".to_vec())
    );
    write(
        &config.join("mimeapps.list"),
        b"[Default Applications]\ntext/plain=lordos-editor.desktop;\n",
    );
    assert_eq!(
        launch::default_for(b"text/plain").unwrap(),
        (1, b"lordos-editor.desktop".to_vec())
    );
    std::fs::remove_file(config.join("mimeapps.list")).unwrap();
    write(
        &root.join("usr/share/mime/globs2"),
        b"50:text/plain:*.txt\n",
    );
    let file = home.join("a ;$(touch injected).txt");
    write(&file, b"hello");
    unsafe {
        assert_eq!(
            launch::apps_launch_with(c"".as_ptr(), cstring(file.as_os_str().as_bytes()).as_ptr()),
            0
        );
        assert_eq!(
            launch::apps_launch_with(c"beta".as_ptr(), c"/etc/hostname".as_ptr()),
            -1
        );
        assert_eq!(
            launch::apps_launch_action(c"beta".as_ptr(), c"missing".as_ptr()),
            -1
        );
        assert_eq!(
            launch::apps_launch_action(c"beta".as_ptr(), c"new".as_ptr()),
            0
        );
        launch::test_restrictions(1 << 9);
        assert_eq!(launch::apps_launch(c"beta".as_ptr()), -1);
        assert_eq!(bytes(launch::apps_last_error()), b"apps.restrictKernel");
        launch::test_restrictions(0);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !home.join("action-marker").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(home.join("action-marker").exists());
    assert!(!home.join("injected").exists());
    // Preferences must not overwrite symlink targets or invalid large files.
    let target = home.join("original");
    write(&target, b"preserve");
    std::os::unix::fs::symlink(&target, config.join("mimeapps.list")).unwrap();
    unsafe {
        assert_eq!(
            defaults::defaults_set(c"text/plain".as_ptr(), c"beta.desktop".as_ptr()),
            -1
        );
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"preserve");
    std::fs::remove_file(config.join("mimeapps.list")).unwrap();
    write(&config.join("mimeapps.list"), &vec![b'x'; 256 * 1024 + 1]);
    unsafe {
        assert_eq!(defaults::defaults_unset(c"text/plain".as_ptr()), -1);
    }
    assert_eq!(
        std::fs::metadata(config.join("mimeapps.list"))
            .unwrap()
            .len(),
        256 * 1024 + 1
    );
}
