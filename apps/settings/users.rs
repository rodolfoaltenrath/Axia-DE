//! Account operations retain the original self/administrator boundaries. Secret
//! arguments are sent only through the quiet administrative channel.
use super::*;
const COLORS: [&str; 8] = [
    "#5b9cff", "#f0915a", "#4cc37a", "#e0609d", "#a77bf3", "#4cc4d3", "#f2c14e", "#8e8e93",
];
pub(super) fn suggest(full: &str) -> String {
    crate::search::fold(full)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(*c))
        .take(32)
        .collect()
}
pub(super) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z' | '_'))
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
}
pub(super) fn add_error(form: &Value) -> &'static str {
    if !valid_name(s(&form["name"])) {
        "settings.users.badName"
    } else if s(&form["pass"]).is_empty() {
        "settings.users.noPass"
    } else if form["pass"] != form["pass2"] {
        "settings.users.mismatch"
    } else if s(&form["full"]).contains([':', ',', '=', '\n']) {
        "settings.users.badFull"
    } else {
        ""
    }
}
pub(super) fn password_error(user: &Value, form: &Value) -> &'static str {
    if s(&form["pass"]).is_empty() {
        ""
    } else if form["pass"] != form["pass2"] {
        "settings.users.mismatch"
    } else if b(&user["self"]) && s(&form["old"]).is_empty() {
        "settings.users.needOld"
    } else {
        ""
    }
}
fn color(user: &Value) -> String {
    user["color"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            let h = s(&user["name"])
                .chars()
                .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
            COLORS[h as usize % 7].into()
        })
}
fn avatar(user: &Value, size: f64) -> Value {
    let c = color(user);
    let name = user["full"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(s(&user["name"]));
    let initials = name
        .split_whitespace()
        .take(2)
        .filter_map(|s| s.chars().next())
        .collect::<String>()
        .to_uppercase();
    view(
        json!({"width":size,"height":size,"borderRadius":size/2.,"overflow":"hidden","alignItems":"center","justifyContent":"center","background":format!("linear-gradient(170deg, {c}, {})",shade(&c,0.78))}),
        vec![if !s(&user["image"]).is_empty() {
            json!({"kind":"image","src":user["image"],"style":{"width":size,"height":size,"objectFit":"cover"}})
        } else {
            json!({"kind":"text","label":initials,"style":{"fontSize":(size*0.4).round(),"fontWeight":600,"color":"#fff"}})
        }],
    )
}
impl App {
    pub(super) fn reconcile_users(&mut self) {
        let user = self.current_user();
        if self.st("detailUser") != &user["name"] {
            self.state["detailUser"] = user["name"].clone();
            self.state["userPassword"] = json!({});
            self.state["userImage"] = json!("");
            self.state["deleteUser"] = json!(false);
            self.state["keepHome"] = json!(true);
        }
        if let Some(name) = self.st("userCreatedPending").as_str() {
            if arr(&self.d("users.list")["users"])
                .iter()
                .any(|u| u["name"] == name)
            {
                self.state["userCreatedPending"] = Value::Null;
            }
        }
        if !self.machine_admin() {
            self.state["addingUser"] = json!(false);
            self.state["newUser"] = Value::Null;
        }
    }
    fn current_user(&self) -> Value {
        let users = arr(&self.d("users.list")["users"]);
        let shown = |u: &&Value| self.machine_admin() || b(&u["self"]);
        users
            .iter()
            .filter(shown)
            .find(|u| u["name"] == self.st("selectedUser").clone())
            .or_else(|| users.iter().find(|u| b(&u["self"])))
            .or_else(|| users.iter().filter(shown).next())
            .cloned()
            .unwrap_or(Value::Null)
    }
    pub(super) fn users(&self) -> Value {
        let u = self.ui();
        let is_admin = self.machine_admin();
        let cur = self.current_user();
        let adding = b(self.st("addingUser"));
        let mut out = vec![];
        if !is_admin {
            out.push(u.note(t("settings.users.onlyOwn"), "warn"));
        }
        let rows = arr(&self.d("users.list")["users"])
            .iter()
            .filter(|user| is_admin || b(&user["self"]))
            .map(|user| {
                let on = user["name"] == cur["name"] && !adding;
                let name = user["full"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(s(&user["name"]));
                let mut row = u.row(
                    format!(
                        "{name}{}",
                        if b(&user["self"]) {
                            format!(" · {}", self.tr("settings.users.you"))
                        } else {
                            String::new()
                        }
                    ),
                    format!(
                        "{} · {}",
                        s(&user["name"]),
                        self.tr(if b(&user["admin"]) {
                            "settings.users.admin"
                        } else {
                            "settings.users.standard"
                        })
                    ),
                    vec![u.text(if on { "✓" } else { "" }, 14., false)],
                );
                row["children"]
                    .as_array_mut()
                    .unwrap()
                    .insert(0, avatar(user, 32.));
                click(row, json!({"op":"userSelect","value":user["name"]}))
            })
            .collect();
        out.push(u.section("", "", rows));
        if is_admin && !adding {
            out.push(actions(vec![u.regular_button(
                t("settings.users.add"),
                op("userAddStart"),
                false,
                false,
            )]));
        }
        if !s(self.st("busy")).is_empty() {
            out.push(u.note(t("settings.users.working"), ""));
        }
        if self.st("userCreatedPending").is_string() {
            out.push(u.note(t("settings.users.working"), ""));
        } else if adding {
            out.push(self.add_user_ui());
        } else if !cur.is_null() {
            out.push(self.user_detail(&cur));
        }
        stack(out)
    }
    fn add_user_ui(&self) -> Value {
        let u = self.ui();
        let f = self.st("newUser");
        let err = add_error(f);
        stack(vec![
            u.section(
                t("settings.users.newTitle"),
                if err.is_empty() { json!("") } else { t(err) },
                vec![
                    u.field(
                        t("settings.users.fullName"),
                        "",
                        "newUser.full",
                        f["full"].as_str().unwrap_or(""),
                        false,
                    ),
                    u.field(
                        t("settings.users.userName"),
                        "",
                        "newUser.name",
                        f["name"].as_str().unwrap_or(""),
                        false,
                    ),
                    u.field(
                        t("settings.users.password"),
                        "",
                        "newUser.pass",
                        f["pass"].as_str().unwrap_or(""),
                        true,
                    ),
                    u.field(
                        t("settings.users.repeat"),
                        "",
                        "newUser.pass2",
                        f["pass2"].as_str().unwrap_or(""),
                        true,
                    ),
                    u.row(
                        t("settings.users.makeAdmin"),
                        t("settings.users.makeAdminHint"),
                        vec![u.toggle(
                            b(&f["admin"]),
                            state("newUser.admin", json!(!b(&f["admin"]))),
                        )],
                    ),
                ],
            ),
            actions(vec![
                u.regular_button(
                    t("settings.users.cancel"),
                    op("userAddCancel"),
                    false,
                    false,
                ),
                u.regular_button(
                    t("settings.users.create"),
                    op("userCreate"),
                    !err.is_empty() || !s(self.st("busy")).is_empty(),
                    true,
                ),
            ]),
        ])
    }
    fn user_detail(&self, user: &Value) -> Value {
        let u = self.ui();
        let admin = self.machine_admin();
        let own = b(&user["self"]);
        let can_edit = admin || own;
        let pw = self.st("userPassword");
        let error = password_error(user, pw);
        let admins = arr(&self.d("users.list")["users"])
            .iter()
            .filter(|x| b(&x["admin"]))
            .count();
        let last_admin = b(&user["admin"]) && admins <= 1;
        let mut sections = vec![view(
            json!({"flexDirection":"row","alignItems":"center","gap":16,"paddingHorizontal":4}),
            vec![
                avatar(user, 64.),
                view(
                    json!({"flex":1,"gap":2}),
                    vec![
                        u.text(
                            user["full"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(s(&user["name"])),
                            20.,
                            false,
                        ),
                        u.text(user["name"].clone(), 12., true),
                    ],
                ),
            ],
        )];
        if can_edit {
            sections.push(u.section(t("settings.users.avatar"),"",vec![view(json!({"flexDirection":"row","flexWrap":"wrap","gap":8,"paddingVertical":12,"direction":"ltr"}),COLORS.iter().map(|c|click(view(json!({"width":22,"height":22,"borderRadius":11,"alignItems":"center","justifyContent":"center","background":format!("linear-gradient(170deg, {c}, {})",shade(c,0.78))}),if user["color"]==*c{vec![view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":"#fff"}),vec![])]}else{vec![]}),json!({"op":"userColor","value":c,"user":user["name"]}))).collect()),view(json!({"flexDirection":"row","gap":8,"paddingVertical":8,"alignItems":"center"}),vec![json!({"kind":"input","value":self.st("userImage").as_str().unwrap_or(""),"placeholder":t("settings.users.imagePath"),"change":["field","userImage"],"style":{"flex":1}}),u.button(t("settings.users.useImage"),op("userImage"),false,false),if !s(&user["image"]).is_empty(){u.button(t("settings.users.removeImage"),op("userRemoveImage"),false,false)}else{Value::Null}])]));
            let mut fields = vec![];
            if own {
                fields.push(u.field(
                    t("settings.users.current"),
                    "",
                    "userPassword.old",
                    pw["old"].as_str().unwrap_or(""),
                    true,
                ));
            }
            fields.push(u.field(
                t("settings.users.newPassword"),
                "",
                "userPassword.pass",
                pw["pass"].as_str().unwrap_or(""),
                true,
            ));
            fields.push(u.field(
                t("settings.users.repeat"),
                "",
                "userPassword.pass2",
                pw["pass2"].as_str().unwrap_or(""),
                true,
            ));
            sections.push(u.section(
                t("settings.users.changePassword"),
                if error.is_empty() {
                    json!("")
                } else {
                    t(error)
                },
                fields,
            ));
            sections.push(actions(vec![u.regular_button(
                t("settings.users.changePassword"),
                op("userPassword"),
                s(&pw["pass"]).is_empty() || !error.is_empty() || !s(self.st("busy")).is_empty(),
                false,
            )]));
        }
        if admin {
            sections.push(u.section(
                "",
                "",
                vec![
                    u.row(
                        t("settings.users.admin"),
                        t(if last_admin {
                            "settings.users.lastAdmin"
                        } else {
                            "settings.users.makeAdminHint"
                        }),
                        vec![u.toggle(b(&user["admin"]), op("userAdmin"))],
                    ),
                    u.row(
                        t("settings.users.autologin"),
                        t("settings.users.autologinHint"),
                        vec![u.toggle(
                            self.d("users.list")["autologin"] == user["name"],
                            op("userAutologin"),
                        )],
                    ),
                ],
            ));
        }
        if admin && !own {
            if b(self.st("deleteUser")) {
                sections.push(u.section("",tp("settings.users.deleteConfirm",json!({"name":user["full"].as_str().filter(|s|!s.is_empty()).unwrap_or(s(&user["name"]))})),vec![u.row(t("settings.users.keepHome"),"",vec![u.toggle(self.st("keepHome")!=false,state("keepHome",json!(self.st("keepHome")==false)))])]));
                sections.push(actions(vec![
                    u.regular_button(
                        t("settings.users.cancel"),
                        state("deleteUser", json!(false)),
                        false,
                        false,
                    ),
                    u.regular_button(t("settings.users.delete"), op("userDelete"), false, false),
                ]));
            } else {
                sections.push(actions(vec![u.regular_button(
                    t("settings.users.delete"),
                    state("deleteUser", json!(true)),
                    false,
                    false,
                )]));
            }
        }
        stack(sections)
    }
    pub(super) fn users_command(&mut self, c: &Value) -> bool {
        let user = self.current_user();
        let name = s(&user["name"]);
        let admin = self.machine_admin();
        let can_edit = admin || b(&user["self"]);
        let busy = !s(self.st("busy")).is_empty();
        match s(&c["op"]) {
            "userSelect" => {
                self.state["selectedUser"] = c["value"].clone();
                self.state["detailUser"] = c["value"].clone();
                self.state["userCreatedPending"] = Value::Null;
                self.state["addingUser"] = json!(false);
                self.state["userPassword"] = json!({});
                self.state["userImage"] = json!("");
                self.state["deleteUser"] = json!(false);
                self.state["keepHome"] = json!(true);
                self.state["message"] = Value::Null;
            }
            "userAddStart" => {
                if admin {
                    self.state["userPassword"] = json!({});
                    self.state["userImage"] = json!("");
                    self.state["deleteUser"] = json!(false);
                    self.state["addingUser"] = json!(true);
                    self.state["newUser"] =
                        json!({"full":"","name":"","pass":"","pass2":"","admin":false});
                    self.state["nameTouched"] = json!(false);
                }
            }
            "userAddCancel" => {
                self.state["addingUser"] = json!(false);
                self.state["newUser"] = Value::Null;
            }
            "userCreate" => {
                if !admin
                    || busy
                    || !b(self.st("addingUser"))
                    || !add_error(self.st("newUser")).is_empty()
                {
                    return true;
                }
                let f = self.st("newUser").clone();
                let arg = format!(
                    "{}\n{}\n{}\n{}",
                    s(&f["name"]),
                    s(&f["full"]).trim(),
                    s(&f["pass"]),
                    if b(&f["admin"]) { 1 } else { 0 }
                );
                self.admin_run("user-add",arg,true,json!({"op":"userCreated","user":f["name"],"success":t("settings.users.created")}));
            }
            "userPassword" => {
                let pw = self.st("userPassword");
                if !can_edit
                    || busy
                    || s(&pw["pass"]).is_empty()
                    || !password_error(&user, pw).is_empty()
                {
                    return true;
                }
                let arg = format!("{name}\n{}\n{}", s(&pw["pass"]), s(&pw["old"]));
                self.admin_run("user-passwd",arg,true,json!({"op":"passwordChanged","user":name,"success":t("settings.users.passwordChanged")}));
            }
            "userColor" | "userImage" | "userRemoveImage" => {
                if !can_edit || busy {
                    return true;
                }
                let mut col = s(&user["color"]).to_owned();
                let image = match s(&c["op"]) {
                    "userColor" => {
                        if !COLORS.contains(&s(&c["value"])) || c["user"] != user["name"] {
                            return true;
                        }
                        col = s(&c["value"]).into();
                        String::new()
                    }
                    "userRemoveImage" => "-".into(),
                    _ => {
                        let p = s(self.st("userImage"));
                        if p.is_empty() {
                            return true;
                        }
                        p.strip_prefix('~')
                            .map(|p| format!("{}{p}", self.home))
                            .unwrap_or_else(|| p.into())
                    }
                };
                self.admin_run(
                    "user-avatar",
                    format!("{name}\n{col}\n{image}"),
                    true,
                    json!({"op":"userChanged","success":t("settings.users.avatarSaved")}),
                );
            }
            "userAdmin" => {
                let last = b(&user["admin"])
                    && arr(&self.d("users.list")["users"])
                        .iter()
                        .filter(|u| b(&u["admin"]))
                        .count()
                        <= 1;
                if !admin || busy || last {
                    return true;
                }
                self.admin_run(
                    "user-admin",
                    format!("{name}\n{}", if b(&user["admin"]) { 0 } else { 1 }),
                    true,
                    json!({"op":"userChanged","success":t("settings.users.saved")}),
                );
            }
            "userAutologin" => {
                if admin && !busy {
                    self.admin_run(
                        "user-autologin",
                        if self.d("users.list")["autologin"] == user["name"] {
                            String::new()
                        } else {
                            name.into()
                        },
                        true,
                        json!({"op":"userChanged","success":t("settings.users.saved")}),
                    );
                }
            }
            "userDelete" => {
                if !admin
                    || busy
                    || b(&user["self"])
                    || !b(self.st("deleteUser"))
                    || name.is_empty()
                {
                    return true;
                }
                self.admin_run(
                    "user-del",
                    format!(
                        "{name}\n{}",
                        if self.st("keepHome") != false { 1 } else { 0 }
                    ),
                    true,
                    json!({"op":"userDeleted","success":t("settings.users.deleted")}),
                );
            }
            _ => return false,
        }
        true
    }
    pub(super) fn users_after(&mut self, after: &Value, _value: &Value) {
        match s(&after["op"]) {
            "userCreated" => {
                self.epoch += 1;
                self.state["addingUser"] = json!(false);
                self.state["selectedUser"] = after["user"].clone();
                self.state["userCreatedPending"] = after["user"].clone();
                self.state["userPassword"] = json!({});
                self.state["userImage"] = json!("");
                self.state["newUser"] = Value::Null;
            }
            "passwordChanged" => {
                if self.current_user()["name"] == after["user"] {
                    self.state["userPassword"] = json!({});
                }
            }
            "userDeleted" => {
                self.epoch += 1;
                self.state["userPassword"] = json!({});
                self.state["userImage"] = json!("");
                self.state["selectedUser"] = Value::Null;
                self.state["deleteUser"] = json!(false);
            }
            _ => {}
        }
    }
}
