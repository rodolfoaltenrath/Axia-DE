//! The sheets over the window: adding and managing accounts (our provider table, app passwords,
//! manual servers, CalDAV/CardDAV), the certificate warning, the password prompt, a link's real
//! address and the color categories. Passwords go to the system keyring, never to a file.
use super::model;
use super::{answer, list_of, str_of, Base, Tag as AppTag};
use serde_json::{json, Value};

pub(crate) enum Tag {
    /// An account was saved; `dav` is its calendar and contacts account, still to be saved.
    Added { dav: Option<(Value, String)> },
    AddedDav { account: String },
    Password,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) enum Step {
    #[default]
    Who,
    Servers,
    Result,
}

#[derive(Default)]
pub(crate) struct Setup {
    /// CalDAV/CardDAV instead of a mail account.
    pub dav: bool,
    pub step: Step,
    pub name: String,
    pub email: String,
    pub password: String,
    pub url: String,
    pub dav_user: String,
    /// The provider's servers (or the guess), editable.
    pub servers: Value,
    pub manual: bool,
    pub with_dav: bool,
    /// The account that was saved and is connecting.
    pub created: Option<String>,
    pub error: String,
    pub busy: bool,
}

#[derive(Default)]
pub(crate) enum Sheet {
    #[default]
    None,
    Setup(Box<Setup>),
    Accounts { selected: String, signature: String },
    Certificate(Value),
    Password { account: String, value: String, busy: bool, error: String },
    Link(Value),
    Labels,
}

/// How a new account is doing, from the status the helper reports.
pub(crate) fn connection(base: &Base, account: &str) -> &'static str {
    let status = &base.facts.status;
    let state = str_of(&status["accounts"][account]["status"]);
    if list_of(&status["certs"]).iter().any(|c| c["acct"] == account) {
        "cert"
    } else if status["authFailed"][account] == true || state == "auth" {
        "auth"
    } else if state == "idle" {
        "ok"
    } else if state == "offline" {
        "offline"
    } else {
        "working"
    }
}

fn taken(base: &Base) -> Vec<String> {
    base.all_accounts().iter().map(|a| str_of(&a["id"]).to_owned()).collect()
}
fn new_id(base: &str, taken: &[String]) -> String {
    model::account_id(base, &taken.iter().map(String::as_str).collect::<Vec<_>>())
}
fn account_call(base: &mut Base, tag: Tag, args: Value) {
    base.service(AppTag::Sheet(tag), "mail.account", args);
}

impl Sheet {
    pub fn setup() -> Self {
        Sheet::Setup(Box::new(Setup { with_dav: true, ..Setup::default() }))
    }
    pub fn accounts(base: &Base) -> Self {
        let first = base.all_accounts().first().cloned().unwrap_or_default();
        Sheet::Accounts { selected: str_of(&first["id"]).to_owned(), signature: str_of(&first["signature"]).to_owned() }
    }
    fn password(account: &str) -> Self {
        Sheet::Password { account: account.to_owned(), value: String::new(), busy: false, error: String::new() }
    }
    /// What the ⚠ beside an account opens: its password, its certificate or the accounts.
    pub fn for_warning(base: &Base, account: &str) -> Self {
        let status = &base.facts.status;
        let state = str_of(&status["accounts"][account]["status"]);
        let certificate = list_of(&status["certs"]).iter().find(|c| c["acct"] == account);
        let dav = base.account(account).is_some_and(|a| a["kind"] == "dav");
        match certificate {
            _ if status["authFailed"][account] == true || state == "auth" => Sheet::password(account),
            Some(certificate) => Sheet::Certificate(certificate.clone()),
            None if dav => Sheet::password(account),
            None => Sheet::accounts(base),
        }
    }

    pub fn facts_changed(&mut self, base: &mut Base) {
        if let Sheet::Accounts { selected, .. } = self {
            if base.account(selected).is_none() {
                *self = Sheet::accounts(base);
            }
        }
    }

    /// A command of the open sheet: `Some(true)` when folders and list have to be read again;
    /// `None` when it is not a sheet's command.
    pub fn command(&mut self, base: &mut Base, command: &str, v: &Value) -> Option<bool> {
        let key = &v["key"];
        let typed = str_of(&v["value"]);
        match command {
            "sheetClose" => {
                let accounts_changed = matches!(self, Sheet::Setup(_) | Sheet::Accounts { .. });
                *self = Sheet::None;
                return Some(accounts_changed);
            }
            "setupKind" => {
                if let Sheet::Setup(setup) = self {
                    setup.dav = key == "dav";
                    setup.error.clear();
                }
            }
            "setupText" => {
                if let Sheet::Setup(setup) = self {
                    setup.text(str_of(key), typed);
                }
            }
            "setupToggle" => {
                if let Sheet::Setup(setup) = self {
                    match str_of(key) {
                        "manual" => setup.manual = !setup.manual,
                        "dav" => setup.with_dav = !setup.with_dav,
                        _ => {}
                    }
                }
            }
            "setupSecurity" => {
                if let Sheet::Setup(setup) = self {
                    let (server, security) = (str_of(&key[0]), str_of(&key[1]));
                    if matches!(server, "imap" | "smtp") && matches!(security, "ssl" | "starttls") {
                        let port = setup.servers[server]["port"].as_u64().unwrap_or(0).min(65535) as u16;
                        setup.servers[server]["security"] = json!(security);
                        setup.servers[server]["port"] = json!(model::port_for(port, security));
                    }
                }
            }
            "setupNext" => {
                if let Sheet::Setup(setup) = self {
                    setup.next(base);
                }
            }
            "setupAdd" => {
                if let Sheet::Setup(setup) = self {
                    setup.save(base);
                }
            }
            "setupRemove" => {
                if let Sheet::Setup(setup) = self {
                    if let Some(id) = setup.created.take() {
                        account_call(base, Tag::Removed, json!(["remove", id]));
                        setup.step = Step::Who;
                    }
                }
            }
            "accountAdd" => *self = Sheet::setup(),
            "accountSelect" => {
                if let Some(account) = key.as_str().and_then(|id| base.account(id)) {
                    *self = Sheet::Accounts { selected: str_of(&account["id"]).to_owned(), signature: str_of(&account["signature"]).to_owned() };
                }
            }
            "signatureText" => {
                if let Sheet::Accounts { signature, .. } = self {
                    *signature = typed.to_owned();
                }
            }
            "signatureSave" => {
                if let Sheet::Accounts { selected, signature } = self {
                    account_call(base, Tag::Removed, json!(["update", selected, {"signature":signature}]));
                }
            }
            "accountPassword" => {
                if let Sheet::Accounts { selected, .. } = self {
                    let id = selected.clone();
                    *self = Sheet::password(&id);
                }
            }
            "accountRemove" => {
                if let Sheet::Accounts { selected, .. } = self {
                    account_call(base, Tag::Removed, json!(["remove", selected]));
                }
            }
            "notifyToggle" => {
                let mut config = base.facts.config.clone();
                config["notify"] = json!(config["notify"] == false);
                base.save_config(config);
            }
            "passwordText" => {
                if let Sheet::Password { value, .. } = self {
                    *value = typed.to_owned();
                }
            }
            "passwordSave" => {
                if let Sheet::Password { account, value, busy, error } = self {
                    if !*busy && !value.is_empty() {
                        *busy = true;
                        error.clear();
                        account_call(base, Tag::Password, json!(["setPassword", account, value]));
                    }
                }
            }
            "certificateTrust" => {
                if let Sheet::Certificate(certificate) = self {
                    // exactly the certificate that was shown, for the server that offered it
                    let id = str_of(&certificate["acct"]).to_owned();
                    let dav = base.account(&id).is_some_and(|a| a["kind"] == "dav");
                    let server = if certificate["smtp"] == true || certificate["smtp"] == 1 {
                        "smtp"
                    } else if dav {
                        "dav"
                    } else {
                        "imap"
                    };
                    account_call(base, Tag::Removed, json!(["trustCertificate", id, server, certificate["sha256"]]));
                    if let Some(waiting) = base.facts.status["certs"].as_array_mut() {
                        waiting.retain(|c| c["acct"] != id.as_str());
                    }
                    *self = Sheet::None;
                }
            }
            "linkCopy" => {
                if let Sheet::Link(link) = self {
                    base.copy(str_of(&link["h"]));
                    *self = Sheet::None;
                }
            }
            _ => return None,
        }
        Some(false)
    }

    pub fn done(&mut self, base: &mut Base, tag: Tag, result: Result<Value, String>, _now: i64) {
        let reply = answer(result);
        match tag {
            Tag::Added { dav } => {
                let Sheet::Setup(setup) = self else { return };
                match reply {
                    Err(failure) => setup.failed(base, &failure),
                    Ok(id) => {
                        let id = str_of(&id).to_owned();
                        match dav {
                            Some((account, password)) => account_call(base, Tag::AddedDav { account: id }, json!(["add", account, password])),
                            None => setup.saved(id),
                        }
                    }
                }
            }
            Tag::AddedDav { account } => {
                if let Sheet::Setup(setup) = self {
                    // the mail account exists either way; a failure of the second one is shown
                    if let Err(failure) = reply {
                        setup.failed(base, &failure);
                    }
                    setup.saved(account);
                }
            }
            Tag::Password => match (reply, &mut *self) {
                (Ok(_), Sheet::Password { .. }) => *self = Sheet::None,
                (Err(failure), Sheet::Password { busy, error, .. }) => {
                    *busy = false;
                    *error = failure.message;
                }
                _ => {}
            },
            Tag::Removed => {}
        }
    }
}

impl Setup {
    fn text(&mut self, field: &str, typed: &str) {
        let typed = typed.to_owned();
        match field {
            "name" => self.name = typed,
            "email" => self.email = typed,
            "password" => self.password = typed,
            "url" => self.url = typed,
            "davUser" => self.dav_user = typed,
            "user" => self.servers["user"] = json!(typed),
            "imapHost" => self.servers["imap"]["host"] = json!(typed),
            "smtpHost" => self.servers["smtp"]["host"] = json!(typed),
            "imapPort" | "smtpPort" => {
                let digits: String = typed.chars().filter(char::is_ascii_digit).collect();
                let port: u16 = digits.parse().unwrap_or(0);
                self.servers[&field[..4]]["port"] = json!(port);
            }
            _ => {}
        }
    }
    fn next(&mut self, base: &mut Base) {
        self.error.clear();
        if self.dav {
            self.save(base);
            return;
        }
        let email = self.email.trim();
        match model::autoconfig(email).filter(|_| model::email_ok(email)) {
            Some(mut servers) => {
                servers["user"] = json!(email);
                self.manual = servers["known"] != true;
                self.servers = servers;
                self.step = Step::Servers;
            }
            None => self.error = base.t("setup.badEmail"),
        }
    }
    fn save(&mut self, base: &mut Base) {
        if self.busy {
            return;
        }
        self.error.clear();
        let mut ids = taken(base);
        if self.dav {
            let url = self.url.trim();
            let Some(rest) = url.get(..8).filter(|s| s.eq_ignore_ascii_case("https://")).map(|_| &url[8..]) else {
                self.error = base.t("setup.badUrl");
                return;
            };
            let name = if self.name.is_empty() { rest.split('/').next().unwrap_or("").to_owned() } else { self.name.clone() };
            let id = new_id(if name.is_empty() { url } else { &name }, &ids);
            let account = json!({"id":id,"kind":"dav","name":name,"url":url,"user":self.dav_user.trim(),"cal":true,"card":true});
            self.busy = true;
            account_call(base, Tag::Added { dav: None }, json!(["add", account, self.password]));
            return;
        }
        let email = self.email.trim().to_owned();
        let servers = &self.servers;
        let user = str_of(&servers["user"]).trim().to_owned();
        let name = if servers["known"] == true { str_of(&servers["name"]).to_owned() } else { email.clone() };
        let id = new_id(&email, &ids);
        ids.push(id.clone());
        let account = json!({"id":id,"kind":"mail","name":name,"email":email,"fullName":self.name.trim(),"user":user,
            "imap":servers["imap"],"smtp":servers["smtp"],"sentSaved":servers["sentSaved"] == true,"auth":"password"});
        let (caldav, carddav) = (str_of(&servers["caldav"]).to_owned(), str_of(&servers["carddav"]).to_owned());
        let dav = (self.with_dav && !(caldav.is_empty() && carddav.is_empty()) && servers["davNote"] != "oauth").then(|| {
            let provider = str_of(&servers["name"]).to_owned();
            let title = format!("{provider} — {}", base.t("setup.calContacts"));
            let account = json!({"id":new_id(&title, &ids),"kind":"dav","name":title,"url":if caldav.is_empty() { &carddav } else { &caldav },
                "user":user,"cal":!caldav.is_empty(),"card":true});
            (account, self.password.clone())
        });
        self.busy = true;
        account_call(base, Tag::Added { dav }, json!(["add", account, self.password]));
    }
    fn saved(&mut self, id: String) {
        self.busy = false;
        self.created = Some(id);
        self.password.clear();
        self.step = Step::Result;
    }
    fn failed(&mut self, base: &mut Base, failure: &super::Failure) {
        self.busy = false;
        self.error = if matches!(failure.code.as_str(), "locked" | "no-keyring") {
            base.t("setup.keyringLocked")
        } else {
            format!("{} {}", base.t("setup.failed"), failure.message)
        };
    }
}
