use super::*;
pub(super) fn vpn_interface(i: &Value) -> bool {
    matches!(s(&i["type"]), "wireguard" | "tun" | "xfrm" | "ppp")
        || ["wg-", "ovpn-", "ips-", "ppp-", "oc-", "fvpn-"]
            .iter()
            .any(|p| s(&i["name"]).starts_with(p))
}
pub(super) fn clean_name(name: &str) -> String {
    let stem = name
        .strip_suffix(".conf")
        .or_else(|| name.strip_suffix(".ovpn"))
        .unwrap_or(name);
    let out: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .take(24)
        .collect();
    if out.is_empty() {
        "vpn".into()
    } else {
        out
    }
}
fn vpn_label(kind: &str) -> &str {
    match kind {
        "wireguard" => "WireGuard",
        "openvpn" => "OpenVPN",
        "ipsec" => "IPsec / IKEv2",
        "pptp" => "PPTP",
        "openconnect" => "Cisco AnyConnect",
        "fortinet" => "Fortinet",
        _ => kind,
    }
}
fn form_fields(kind: &str, form: &Value) -> Vec<(&'static str, bool, bool)> {
    match kind {
        "ipsec" => {
            let mut v = vec![("server", false, false), ("auth", false, false)];
            if form["auth"] == "psk" {
                v.push(("psk", true, false));
            } else {
                v.extend([("user", false, false), ("password", true, false)]);
            }
            v.extend([("remote_id", false, true), ("routes", false, true)]);
            v
        }
        "pptp" => vec![
            ("server", false, false),
            ("user", false, false),
            ("password", true, false),
            ("routes", false, true),
        ],
        "openconnect" => vec![
            ("server", false, false),
            ("protocol", false, false),
            ("user", false, false),
            ("password", true, false),
            ("usergroup", false, true),
            ("servercert", false, true),
        ],
        "fortinet" => vec![
            ("host", false, false),
            ("port", false, true),
            ("user", false, false),
            ("password", true, false),
            ("trusted-cert", false, true),
            ("routes", false, true),
        ],
        _ => vec![],
    }
}
impl App {
    pub(super) fn set_network_page(&mut self, page: &str) {
        if self.st("networkPage") == page {
            return;
        }
        self.close_network();
        self.epoch += 1;
        self.state["networkPage"] = json!(page);
        self.state["interfaceForm"] = Value::Null;
        self.data["interfaceConfig"] = Value::Null;
        self.data["interfaceConfigName"] = Value::Null;
        self.state["message"] = Value::Null;
        self.read_at = 0;
    }
    pub(super) fn vpn_autofill_tree(&self) -> Value {
        let u = self.ui();
        let items = arr(self.st("vpnAutofillItems"));
        let mut rows = vec![u.text(t("passwords.autofill.title"), 11., true)];
        if self.st("vpnAutofillPending") == 0 && items.is_empty() {
            rows.push(u.text(t("passwords.autofill.none"), 12., true));
        }
        for (index, item) in items.iter().enumerate() {
            let mut labels = vec![u.text(item["title"].clone(), 13., false)];
            if !s(&item["username"]).is_empty() {
                labels.push(u.text(item["username"].clone(), 11., true));
            }
            rows.push(click(
                view(json!({"padding":6,"borderRadius":6}), labels),
                json!({"op":"vpnAutofillPick","index":index}),
            ));
        }
        view(
            json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":1000}),
            vec![
                click(
                    view(
                        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0}),
                        vec![],
                    ),
                    state("vpnAutofillOpen", json!(false)),
                ),
                json!({"kind":"view","key":"vpn-autofill-popup","layer":"popover","fitPopover":true,"anchorKey":"vpn-autofill-button","style":{"position":"absolute","top":34,"width":260,"padding":4,"borderRadius":9,"backgroundColor":u.color("windowBg"),"border":format!("1px solid {}",s(&u.color("separator"))),"boxShadow":"0 10px 28px rgba(0,0,0,.4)"},"children":rows}),
            ],
        )
    }
    fn autofill_complete(&mut self, after: &Value, value: Option<&Value>) {
        if self.st("vpnAuth")["name"] != after["name"] || self.st("vpnAutofillOpen") != true {
            return;
        }
        let mut items = arr(self.st("vpnAutofillItems")).to_vec();
        if let Some(value) = value {
            if after["op"] == "vpnAutofillKeyring" && !value.is_null() {
                items.push(json!({"kind":"kr","title":value["label"].as_str().filter(|s|!s.is_empty()).map(String::from).unwrap_or_else(||format!("vpn:{}",s(&after["name"]))),"username":value["account"],"secret":value["secret"]}));
            } else if after["op"] == "vpnAutofillEntries" {
                for entry in arr(value) {
                    let mut entry = entry.clone();
                    entry["kind"] = json!("pw");
                    items.push(entry);
                }
            }
        }
        self.state["vpnAutofillItems"] = json!(items);
        self.state["vpnAutofillPending"] = json!((n(self.st("vpnAutofillPending")) - 1.).max(0.));
    }
    pub(super) fn network_failed(&mut self, after: &Value, _result: &Value) -> bool {
        if matches!(s(&after["op"]), "vpnAutofillKeyring" | "vpnAutofillEntries") {
            self.autofill_complete(after, None);
            true
        } else {
            after["op"] == "vpnAutofillRevealed"
        }
    }
    pub(super) fn network_action(&mut self, v: &Value) {
        let Ok(action) = serde_json::from_str::<Value>(s(&v["key"])) else {
            return;
        };
        if let Ok(tree) = crate::network::request(
            json!({"op":"action","id":self.st("networkPanel"),"theme":self.theme,"action":action,"value":v["value"]}),
        ) {
            if tree.is_object() {
                self.data["networkTree"] = adapt(tree);
            }
        }
    }
    pub(super) fn poll_network(&mut self) {
        if self.st("networkPanel").is_null() {
            if let Ok(value) = crate::network::request(json!({"op":"create","compact":false})) {
                self.state["networkPanel"] = value["id"].clone();
            }
        }
        if let Ok(tree) = crate::network::request(
            json!({"op":"poll","id":self.st("networkPanel"),"theme":self.theme}),
        ) {
            if tree.is_object() {
                self.data["networkTree"] = adapt(tree);
            }
        }
    }
    pub(super) fn close_network(&mut self) {
        if !self.st("networkPanel").is_null() {
            let _ = crate::network::request(json!({"op":"close","id":self.st("networkPanel")}));
            self.state["networkPanel"] = Value::Null;
            self.data["networkTree"] = Value::Null;
        }
    }
    pub(super) fn network(&self) -> Value {
        let u = self.ui();
        if self.loaded && self.d("net.interfaces").is_null() {
            return stack(vec![u.note(t("network.unavailable"), "warn")]);
        }
        let page = s(self.st("networkPage"));
        let mut tabs: Vec<Value> = arr(self.d("net.interfaces"))
            .iter()
            .filter(|i| !vpn_interface(i) && i["type"] != "loopback")
            .map(|i| json!([format!("if:{}", s(&i["name"])), i["name"]]))
            .collect();
        tabs.extend([json!(["wifi", "Wi-Fi"]), json!(["vpn", "VPN"])]);
        let body = if let Some(name) = page.strip_prefix("if:") {
            self.interface(name)
        } else if page == "vpn" {
            self.vpn()
        } else {
            stack(vec![u.section(
                "Wi-Fi",
                "",
                vec![self.d("networkTree").clone()],
            )])
        };
        view(
            json!({"gap":20}),
            vec![
                view(
                    json!({"alignItems":"center"}),
                    vec![u.tabs(json!(page), tabs, op("networkPage"))],
                ),
                body,
            ],
        )
    }
    fn interface(&self, name: &str) -> Value {
        let u = self.ui();
        let Some(i) = arr(self.d("net.interfaces"))
            .iter()
            .find(|i| i["name"] == name)
        else {
            return u.note(tp("network.gone", json!({"name":name})), "");
        };
        let global = arr(&i["addresses"]).iter().any(|a| a["scope"] == "global");
        let status = if !b(&i["up"]) {
            "off"
        } else if !b(&i["carrier"]) && !matches!(s(&i["type"]), "wireguard" | "tun") {
            "noCable"
        } else if global {
            "connected"
        } else {
            "noAddress"
        };
        let sys = i["managed"] == "system";
        let mut out = vec![u.section(
            "",
            "",
            vec![u.row(
                name,
                format!(
                    "{} · {}",
                    self.tr(&format!("network.type.{}", s(&i["type"]))),
                    self.tr(&format!("network.status.{status}"))
                ),
                if !sys && self.admin() {
                    vec![u.toggle(
                        b(&i["up"]),
                        json!({"op":"netLink","name":name,"on":!b(&i["up"])}),
                    )]
                } else {
                    vec![]
                },
            )],
        )];
        let mut details = vec![];
        for a in arr(&i["addresses"]).iter().filter(|a| a["scope"] != "host") {
            details.push(u.value(
                if a["family"] == 4 { "IPv4" } else { "IPv6" },
                format!(
                    "{}/{}{}",
                    s(&a["address"]),
                    a["prefix"],
                    if b(&a["dynamic"]) {
                        format!(" · {}", self.tr("network.dynamic"))
                    } else {
                        String::new()
                    }
                ),
            ));
        }
        if let Some(gw) = arr(self.d("net.routes"))
            .iter()
            .find(|r| r["dst"] == "default" && r["dev"] == name)
        {
            details.push(u.value(t("network.gateway"), gw["gateway"].clone()));
            let dns = arr(&self.d("net.dns")["servers"])
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            if !dns.is_empty() {
                details.push(u.value("DNS", dns));
            }
        }
        if !s(&i["mac"]).is_empty() && i["type"] != "wireguard" {
            details.push(u.value("MAC", i["mac"].clone()));
        }
        details.push(u.value(
            t("network.traffic"),
            tp(
                "network.trafficValue",
                json!({"rx":self.bytes(n(&i["rxBytes"])),"tx":self.bytes(n(&i["txBytes"]))}),
            ),
        ));
        out.push(u.section("", "", details));
        if sys {
            out.push(u.section(
                t("network.config"),
                t("network.systemManaged"),
                if self.admin() {
                    vec![u.row(
                        t("network.manage"),
                        "",
                        vec![u.button(
                            t("network.manage"),
                            json!({"op":"netManage","name":name}),
                            false,
                            false,
                        )],
                    )]
                } else {
                    vec![]
                },
            ));
        } else if matches!(s(&i["type"]), "wireguard" | "tun") {
            out.push(u.section(t("network.config"), t("network.vpnInterface"), vec![]));
        } else {
            let form = self.interface_form(name);
            let mut fields = vec![u.row(
                t("network.method"),
                "",
                vec![u.segmented(
                    form["method"].clone(),
                    opts(&["dhcp", "static", "off"], "network."),
                    state("interfaceForm.method", Value::Null),
                )],
            )];
            if form["method"] == "static" {
                for (key, label) in [
                    ("address", "network.address"),
                    ("gateway", "network.gateway"),
                    ("dns", "DNS"),
                ] {
                    fields.push(u.field(
                        if label == "DNS" {
                            json!(label)
                        } else {
                            t(label)
                        },
                        "",
                        &format!("interfaceForm.{key}"),
                        form[key].clone(),
                        false,
                    ));
                }
            }
            out.push(u.section(
                t("network.config"),
                if self.admin() {
                    json!("")
                } else {
                    t("network.needAdmin")
                },
                fields,
            ));
            let mut buttons = vec![];
            if i["managed"] == "lordos" && self.admin() && !self.d("interfaceConfig").is_null() {
                buttons.push(u.regular_button(
                    t("network.unmanage"),
                    json!({"op":"netUnmanage","name":name}),
                    false,
                    false,
                ));
            }
            buttons.push(u.regular_button(
                t("network.apply"),
                json!({"op":"netApply","name":name}),
                !self.admin() || self.d("interfaceConfigName") != name,
                true,
            ));
            out.push(actions(buttons));
        }
        stack(out)
    }
    fn interface_form(&self, _name: &str) -> Value {
        let saved = self.d("interfaceConfig");
        let mut form = json!({"method":saved["method"].as_str().unwrap_or("dhcp"),"address":saved["addresses"][0].as_str().unwrap_or(""),"gateway":saved["gateway"].as_str().unwrap_or(""),"dns":arr(&saved["dns"]).iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")});
        merge(&mut form, self.st("interfaceForm"));
        form
    }
    fn vpn(&self) -> Value {
        let u = self.ui();
        let mut out = vec![];
        if b(self.st("vpnImport")) {
            out.push(self.vpn_import());
        }
        if self.st("vpnAuth").is_object() {
            let auth = self.st("vpnAuth");
            let mut fields = vec![
                u.field(
                    t("network.vpn.user"),
                    "",
                    "vpnAuth.user",
                    auth["user"].clone(),
                    false,
                ),
                u.field(
                    t("network.vpn.password"),
                    "",
                    "vpnAuth.pass",
                    auth["pass"].clone(),
                    true,
                ),
            ];
            let mut key = u.button("🔑", op("vpnAutofill"), false, false);
            key["key"] = json!("vpn-autofill-button");
            merge(
                &mut key["style"],
                &json!({"width":30,"height":30,"paddingHorizontal":0,"borderRadius":7}),
            );
            key["children"][0]["style"]["fontSize"] = json!(14);
            fields[1]["children"][1]["children"][0]["style"]["width"] = json!(202);
            fields[1]["children"][1]["children"]
                .as_array_mut()
                .unwrap()
                .push(key);
            out.push(u.section(
                tp("network.vpn.authTitle", json!({"name":auth["name"]})),
                "",
                fields,
            ));
            out.push(actions(vec![
                u.regular_button(
                    t("network.cancel"),
                    state("vpnAuth", Value::Null),
                    false,
                    false,
                ),
                u.regular_button(
                    t("network.vpn.saveConnect"),
                    op("vpnCredentials"),
                    false,
                    true,
                ),
            ]));
        }
        let list = arr(self.d("net.vpnList"));
        let mut rows = vec![];
        let add_button = if self.admin() && !b(self.st("vpnImport")) {
            u.button(t("network.vpn.add"), op("vpnImportStart"), false, false)
        } else {
            view(json!({}), vec![])
        };
        for v in list {
            let status = &self.data["vpnStatus"][s(&v["name"])];
            let on = b(&status["active"]);
            let mut controls = vec![];
            if self.admin() && v["type"] == "openvpn" && b(&v["needsAuth"]) {
                controls.push(u.button(
                    t("network.vpn.credentials"),
                    json!({"op":"vpnAuthStart","name":v["name"]}),
                    false,
                    false,
                ));
            }
            if self.admin() {
                controls.push(u.button(
                    t("network.vpn.delete"),
                    json!({"op":"vpnDelete","name":v["name"]}),
                    false,
                    false,
                ));
            }
            controls.push(u.toggle(
                on || b(&status["running"]),
                json!({"op":"vpnToggle","name":v["name"],"on":!on&&!b(&status["running"])}),
            ));
            let hint = format!(
                "{} · {}{}{}{}",
                vpn_label(s(&v["type"])),
                s(&v["server"]),
                if !s(&v["user"]).is_empty() {
                    format!(" · {}", s(&v["user"]))
                } else {
                    String::new()
                },
                if b(&v["fullTunnel"]) && v["type"] != "openconnect" {
                    format!(" · {}", self.tr("network.vpn.fullTunnel"))
                } else {
                    String::new()
                },
                if b(&v["insecure"]) {
                    format!(" · ⚠ {}", self.tr("network.vpn.insecure"))
                } else {
                    String::new()
                }
            );
            let mut lines = vec![hint];
            let peer = &status["peers"][0];
            if on && n(&peer["lastHandshake"]) > 0. {
                lines.push(self.caption(&tp("network.vpn.handshake",json!({"s":(self.now as f64/1000.-n(&peer["lastHandshake"])).round().max(0.),"rx":self.bytes(n(&peer["rxBytes"])),"tx":self.bytes(n(&peer["txBytes"]))}))));
            }
            if on && v["type"] != "wireguard" {
                if let Some(iface) = arr(self.d("net.interfaces"))
                    .iter()
                    .find(|i| i["name"] == v["ifname"])
                {
                    let ip = arr(&iface["addresses"])
                        .iter()
                        .find(|a| a["family"] == 4)
                        .map(|a| s(&a["address"]))
                        .unwrap_or("");
                    lines.push(format!("{ip} · {}",self.caption(&tp("network.trafficValue",json!({"rx":self.bytes(n(&iface["rxBytes"])),"tx":self.bytes(n(&iface["txBytes"]))})))));
                }
            }
            if v["type"] == "wireguard" && !s(&v["publicKey"]).is_empty() {
                lines.push(
                    self.caption(&tp("network.vpn.publicKey", json!({"key":v["publicKey"]}))),
                );
            }
            let mut row = u.row(v["name"].clone(), lines.join("\n"), controls);
            row["style"]["minHeight"] = json!(52);
            row["children"][0]["children"][0]["style"]["fontWeight"] = json!(600);
            rows.push(u.row_icon(row,view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":if on{"#30d158"}else if b(&status["running"]){"#ff9f0a"}else{"#8e8e93"}}),vec![])));
        }
        if list.is_empty() {
            rows.push(u.row(t("network.vpn.none"), "", vec![]));
        }
        out.push(u.accessory(u.section("VPN", "", rows), add_button));
        stack(out)
    }
    fn vpn_import(&self) -> Value {
        let u = self.ui();
        let kind = s(self.st("vpnType"));
        let types = arr(self.d("net.vpnTypes"));
        let current = types.iter().find(|v| v["type"] == kind);
        let mut rows=vec![view(json!({"flexDirection":"row","flexWrap":"wrap","gap":8,"paddingVertical":12}),types.iter().map(|tp|{let on=tp["type"]==kind;let mut kids=vec![u.text(vpn_label(s(&tp["type"])),12.,false)];if b(&tp["optional"])||b(&tp["insecure"]){kids.push(u.text(t(if b(&tp["insecure"]){"network.vpn.insecure"}else if b(&tp["available"]){"network.vpn.optionalOn"}else{"network.vpn.optionalOff"}),10.,true));}click(view(json!({"minWidth":112,"minHeight":44,"justifyContent":"center","paddingHorizontal":10,"paddingVertical":7,"borderRadius":8,"gap":2,"backgroundColor":u.color(if on{"accent"}else{"control"})}),kids),json!({"op":"vpnType","value":tp["type"]}))}).collect())];
        let file_type = matches!(kind, "wireguard" | "openvpn");
        let available = current.is_some_and(|v| b(&v["available"]));
        if let Some(cur) = current {
            if b(&cur["optional"]) && !available {
                rows.push(u.row(
                    vpn_label(kind),
                    tp(
                        "network.vpn.enableHint",
                        json!({"name":vpn_label(kind),"pkg":cur["package"]}),
                    ),
                    vec![u.button(
                        t(if self.st("busy") == "net-vpn-enable" {
                            "network.vpn.enabling"
                        } else {
                            "network.vpn.enable"
                        }),
                        op("vpnEnable"),
                        !s(self.st("busy")).is_empty(),
                        true,
                    )],
                ));
            }
        }
        if available {
            if file_type {
                rows.push(u.note(t("network.vpn.importHint"), ""));
                let files = arr(self.d("vpnFiles"));
                let shown: Vec<_> = files
                    .iter()
                    .filter(|f| {
                        s(&f["rel"]).to_lowercase().ends_with(if kind == "openvpn" {
                            ".ovpn"
                        } else {
                            ".conf"
                        })
                    })
                    .collect();
                for file in &shown {
                    rows.push(click(
                        u.row(format!("~/{}", s(&file["rel"])), "", vec![]),
                        json!({"op":"vpnFile","value":file}),
                    ));
                }
                if shown.is_empty() {
                    rows.push(u.note(t("network.vpn.noFiles"), ""));
                }
            } else {
                for (field, secret, optional) in form_fields(kind, self.st("vpnForm")) {
                    if matches!(field, "auth" | "protocol") {
                        rows.push(u.row(
                            t(&format!("network.vpn.field.{field}")),
                            "",
                            vec![u.segmented(
                                self.st("vpnForm")[field].clone(),
                                opts(
                                    if field == "auth" {
                                        &["eap", "psk"]
                                    } else {
                                        &["anyconnect", "gp", "pulse"]
                                    },
                                    "network.vpn.opt.",
                                ),
                                state(&format!("vpnForm.{field}"), Value::Null),
                            )],
                        ));
                    } else {
                        rows.push(u.field(
                            t(&format!("network.vpn.field.{field}")),
                            if optional {
                                t("network.vpn.optional")
                            } else {
                                json!("")
                            },
                            &format!("vpnForm.{field}"),
                            self.st("vpnForm")[field].as_str().unwrap_or(""),
                            secret,
                        ));
                    }
                }
            }
            if !file_type || self.st("vpnFile").is_object() {
                rows.push(u.field(
                    t("network.vpn.name"),
                    "",
                    "vpnName",
                    self.st("vpnName").as_str().unwrap_or(""),
                    false,
                ));
            }
        }
        let mut out = vec![u.section(t("network.vpn.importTitle"), "", rows)];
        if current.is_some_and(|c| b(&c["insecure"])) {
            out.push(u.note(t("network.vpn.pptpWarning"), "warn"));
        }
        let mut buttons = vec![u.regular_button(
            t("network.close"),
            state("vpnImport", json!(false)),
            false,
            false,
        )];
        if available && (!file_type || self.st("vpnFile").is_object()) {
            buttons.push(u.regular_button(t("network.vpn.import"), op("vpnImport"), false, true));
        }
        out.push(actions(buttons));
        stack(out)
    }
    fn vpn_status(&mut self) {
        self.state["vpnAt"] = json!(self.now + 4000);
        self.call(
            "admin.run",
            json!(["net-vpn-status", "", true]),
            json!({"op":"vpnStatus"}),
        );
    }
    pub(super) fn network_command(&mut self, c: &Value) -> bool {
        match s(&c["op"]) {
            "networkPage" => {
                self.set_network_page(s(&c["value"]));
                if c["value"] == "vpn" {
                    self.vpn_status();
                }
            }
            "netLink" => self.admin_run(
                if b(&c["on"]) { "net-up" } else { "net-down" },
                s(&c["name"]).into(),
                true,
                json!({}),
            ),
            "netManage" => self.admin_run(
                "net-manage",
                s(&c["name"]).into(),
                true,
                json!({"success":t("network.managedNow")}),
            ),
            "netUnmanage" => self.admin_run("net-unmanage", s(&c["name"]).into(), true, json!({})),
            "netApply" => {
                if !self.admin()
                    || self.d("interfaceConfigName") != s(&c["name"])
                    || s(self.st("networkPage")) != format!("if:{}", s(&c["name"]))
                {
                    return true;
                }
                let name = s(&c["name"]);
                let f = self.interface_form(name);
                let mut lines = vec![name.into(), format!("method={}", s(&f["method"]))];
                if f["method"] == "static" {
                    for key in ["address", "gateway"] {
                        if !s(&f[key]).is_empty() {
                            lines.push(format!("{key}={}", s(&f[key]).trim()));
                        }
                    }
                    for dns in s(&f["dns"])
                        .split(|c: char| c.is_whitespace() || c == ',')
                        .filter(|s| !s.is_empty())
                    {
                        lines.push(format!("dns={dns}"));
                    }
                }
                self.admin_run(
                    "net-config",
                    lines.join("\n"),
                    true,
                    json!({"op":"interfaceApplied","success":t("network.applied")}),
                );
            }
            "vpnImportStart" => {
                self.state["vpnImport"] = json!(true);
                let mut found = vec![];
                for dir in [
                    "",
                    "Descargas",
                    "Downloads",
                    "Documentos",
                    "Documents",
                    "Escritorio",
                    "Desktop",
                ] {
                    for entry in std::fs::read_dir(std::path::Path::new(&self.home).join(dir))
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                    {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if entry
                            .metadata()
                            .is_ok_and(|m| m.is_file() && m.len() < 256 * 1024)
                            && [".conf", ".ovpn"]
                                .iter()
                                .any(|ext| name.to_lowercase().ends_with(ext))
                        {
                            found.push(json!({"name":name,"rel":if dir.is_empty(){name.clone()}else{format!("{dir}/{name}")}}));
                        }
                    }
                }
                self.data["vpnFiles"] = json!(found);
            }
            "vpnType" => {
                self.state["vpnType"] = c["value"].clone();
                self.state["vpnForm"] = match s(&c["value"]) {
                    "ipsec" => json!({"auth":"eap"}),
                    "openconnect" => json!({"protocol":"anyconnect"}),
                    "fortinet" => json!({"port":"443"}),
                    _ => json!({}),
                };
                self.state["vpnFile"] = Value::Null;
                self.state["vpnName"] = json!("");
            }
            "vpnFile" => {
                self.state["vpnFile"] = c["value"].clone();
                self.state["vpnName"] = json!(clean_name(s(&c["value"]["name"])));
            }
            "vpnEnable" => self.admin_run(
                "net-vpn-enable",
                s(self.st("vpnType")).into(),
                true,
                json!({}),
            ),
            "vpnImport" => self.import_vpn(),
            "vpnToggle" => {
                self.admin_run(
                    if b(&c["on"]) {
                        "net-vpn-up"
                    } else {
                        "net-vpn-down"
                    },
                    s(&c["name"]).into(),
                    true,
                    json!({"op":"vpnChanged"}),
                );
            }
            "vpnDelete" => self.admin_run(
                "net-vpn-delete",
                s(&c["name"]).into(),
                true,
                json!({"op":"vpnChanged"}),
            ),
            "vpnAuthStart" => self.state["vpnAuth"] = json!({"name":c["name"],"user":"","pass":""}),
            "vpnCredentials" => {
                let a = self.state["vpnAuth"].take();
                if !a.is_object() {
                    return true;
                }
                if !s(&a["pass"]).is_empty() {
                    self.call("keyring.set",json!(["settings",{"service":format!("vpn:{}",s(&a["name"])),"account":a["user"],"secret":a["pass"],"label":format!("VPN {}",s(&a["name"]))}]),Value::Null);
                }
                self.admin_run(
                    "net-vpn-credentials",
                    format!("{}\n{}\n{}", s(&a["name"]), s(&a["user"]), s(&a["pass"])),
                    true,
                    json!({"op":"vpnCredentialsSaved","name":a["name"]}),
                );
            }
            "vpnAutofill" => {
                let open = self.st("vpnAutofillOpen") != true;
                self.state["vpnAutofillOpen"] = json!(open);
                if open && self.st("vpnAuth").is_object() {
                    let name = self.st("vpnAuth")["name"].clone();
                    self.state["vpnAutofillItems"] = json!([]);
                    self.state["vpnAutofillPending"] = json!(2);
                    let service = format!("vpn:{}", s(&name));
                    self.call(
                        "keyring.get",
                        json!(["settings",{"service":service}]),
                        json!({"op":"vpnAutofillKeyring","name":name}),
                    );
                    self.call(
                        "autofill.list",
                        json!(["settings",{"service":service}]),
                        json!({"op":"vpnAutofillEntries","name":name}),
                    );
                }
            }
            "vpnAutofillPick" => {
                let index = c["index"].as_u64().unwrap_or(u64::MAX) as usize;
                let item = arr(self.st("vpnAutofillItems")).get(index).cloned();
                if let Some(item) = item {
                    self.state["vpnAutofillOpen"] = json!(false);
                    if item["kind"] == "kr" {
                        if !s(&item["username"]).is_empty() {
                            self.state["vpnAuth"]["user"] = item["username"].clone();
                        }
                        self.state["vpnAuth"]["pass"] = item["secret"].clone();
                        self.state["vpnAutofillItems"] = Value::Null;
                    } else {
                        self.call(
                            "autofill.reveal",
                            json!([item["id"]]),
                            json!({"op":"vpnAutofillRevealed","name":self.st("vpnAuth")["name"]}),
                        );
                    }
                }
            }
            _ => return false,
        }
        true
    }
    fn import_vpn(&mut self) {
        let kind = s(self.st("vpnType")).to_owned();
        let name = clean_name(
            self.st("vpnName")
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.st("vpnFile")["name"].as_str().unwrap_or(&kind)),
        );
        if matches!(kind.as_str(), "openvpn" | "wireguard") {
            let path = s(&self.st("vpnFile")["rel"]);
            if !arr(self.d("vpnFiles")).iter().any(|f| f["rel"] == path) {
                return;
            }
            let path = std::path::Path::new(&self.home).join(path);
            if kind == "openvpn" {
                self.call(
                    "net.prepareOvpn",
                    json!([path]),
                    json!({"op":"vpnPrepared","kind":kind,"name":name}),
                );
            } else if let Ok(config) = std::fs::read_to_string(path) {
                self.admin_run(
                    "net-vpn-import",
                    format!("{kind}\n{name}\n{config}"),
                    true,
                    json!({"op":"vpnImported","success":t("network.vpn.imported")}),
                );
            }
            return;
        }
        let form = self.st("vpnForm");
        let fields = form_fields(&kind, form);
        for (field, _, optional) in &fields {
            if !optional && s(&form[field]).trim().is_empty() {
                self.message(
                    false,
                    tp(
                        "network.vpn.missing",
                        json!({"field":self.tr(&format!("network.vpn.field.{field}"))}),
                    ),
                );
                return;
            }
        }
        let config = fields
            .iter()
            .filter(|(k, _, _)| !s(&form[k]).trim().is_empty())
            .map(|(k, _, _)| format!("{k}={}", s(&form[k]).trim()))
            .collect::<Vec<_>>()
            .join("\n");
        self.admin_run(
            "net-vpn-import",
            format!("{kind}\n{name}\n{config}"),
            true,
            json!({"op":"vpnImported","success":t("network.vpn.imported")}),
        );
    }
    pub(super) fn network_after(&mut self, after: &Value, value: &Value, log: &Value) {
        match s(&after["op"]) {
            "vpnStatus" => {
                let mut status = json!({});
                for line in arr(log) {
                    if let Ok(v) = serde_json::from_str::<Value>(s(line)) {
                        if let Some(name) = v["name"].as_str() {
                            status[name] = v.clone();
                        }
                    }
                }
                self.data["vpnStatus"] = status;
            }
            "vpnChanged" => self.vpn_status(),
            "vpnCredentialsSaved" => self.admin_run(
                "net-vpn-up",
                s(&after["name"]).into(),
                true,
                json!({"op":"vpnChanged"}),
            ),
            "vpnPrepared" => self.admin_run(
                "net-vpn-import",
                format!("{}\n{}\n{}", s(&after["kind"]), s(&after["name"]), s(value)),
                true,
                json!({"op":"vpnImported","success":t("network.vpn.imported")}),
            ),
            "vpnImported" => {
                self.state["vpnImport"] = json!(false);
                self.state["vpnForm"] = json!({});
                self.vpn_status();
            }
            "vpnAutofillKeyring" | "vpnAutofillEntries" => {
                self.autofill_complete(after, Some(value))
            }
            "vpnAutofillRevealed" => {
                if self.st("vpnAuth")["name"] == after["name"] {
                    if !s(&value["username"]).is_empty() {
                        self.state["vpnAuth"]["user"] = value["username"].clone();
                    }
                    self.state["vpnAuth"]["pass"] = value["password"].clone();
                    self.state["vpnAutofillItems"] = Value::Null;
                }
            }
            "interfaceApplied" => self.state["interfaceForm"] = Value::Null,
            _ => {}
        }
    }
    pub(super) fn network_event(&mut self, event: &Value) {
        if event["kind"] != "admin" {
            return;
        }
        let text = s(&event["text"]);
        let Some(tail) = text.strip_prefix("net vpn ") else {
            return;
        };
        let mut parts = tail.splitn(3, ' ');
        let name = parts.next().unwrap_or("");
        let kind = parts.next().unwrap_or("");
        match kind {
            "need-auth" => self.state["vpnAuth"] = json!({"name":name,"user":"","pass":""}),
            "connected" => self.message(true, tp("network.vpn.connectedMsg", json!({"name":name}))),
            "auth-failed" => {
                self.message(false, tp("network.vpn.authFailed", json!({"name":name})))
            }
            "error" | "log" => self.message(false, json!(parts.next().unwrap_or(""))),
            _ => {}
        }
        self.read_at = 0;
        self.vpn_status();
    }
}
fn adapt(mut node: Value) -> Value {
    if !node.is_object() {
        return node;
    }
    let kind = node["type"].clone();
    node["kind"] = kind.clone();
    if kind == "text" {
        node["label"] = node["text"].clone();
    }
    let wrap = |a: &Value| json!(["networkAction", a.to_string()]);
    if kind == "input" {
        node["change"] = wrap(&node["action"]);
    } else if kind == "button" {
        node["action"] = wrap(&node["action"]);
    }
    for key in ["submit", "cancel"] {
        if node[key].is_array() {
            node[key] = wrap(&node[key]);
        }
    }
    if let Some(events) = node["events"].as_object_mut() {
        for (_, a) in events {
            *a = wrap(a);
        }
    }
    if let Some(children) = node["children"].as_array_mut() {
        for child in children {
            *child = adapt(child.take());
        }
    }
    node
}
