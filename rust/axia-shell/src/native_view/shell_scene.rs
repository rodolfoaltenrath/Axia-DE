//! Session-owned shell mounts. React supplies the root identity once; native
//! services create their own hosts, actions and lifetimes below shared layers.
use super::{Host, switcher::Switcher};
use crate::{runtime::native as scene, winmodel::{Session, WeakSession, SwitcherOpening}};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
#[path = "shell_swipe.rs"]
mod swipe;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Node(i32, u64);
impl Node {
    fn bind(id: i32) -> Option<Self> { scene::generation(id).map(|generation| Self(id,generation)) }
    fn live(self) -> bool { scene::generation(self.0) == Some(self.1) }
}

#[cfg(test)]
#[path = "shell_scene_tests.rs"]
mod tests;
struct Mount { nonce: u64, handle: u32, node: Node }
impl Drop for Mount {
    fn drop(&mut self) {
        if super::VIEWS.try_with(|_|()).is_ok() { super::axia_view_unmount(self.handle); }
        if scene::alive() && self.node.live() { scene::destroy(self.node.0); }
    }
}
pub(crate) struct Service {
    owner: WeakSession,
    root: Option<Node>,
    layers: BTreeMap<i32, Node>,
    opening: Reader<Option<SwitcherOpening>>,
    changes: Subscription,
    mounted: Option<Mount>,
    hud: Option<Mount>,
    swipe: swipe::Swipe,
    pending: VecDeque<(u64, Value)>,
    input: super::shell_input::Input,
    commands: crate::forcequit::App,
}
impl Service {
    pub(crate) fn new(owner: &Session) -> Self {
        let opening = owner.switcher_reader();
        let changes = opening.subscribe();
        let home = super::resource_roots().1.unwrap_or_default();
        Self { owner: owner.downgrade(), root: None, layers: BTreeMap::new(),
            opening, changes, mounted: None, hud: None, swipe: Default::default(), pending: VecDeque::new(), input: super::shell_input::Input::new(&home),
            commands: crate::forcequit::App::with_window_metadata(std::rc::Rc::new(crate::winmodel::WindowReader(owner.reader())),owner.catalog_store()) }
    }
    fn root_live(&self) -> bool {
        self.root.is_some_and(|root| scene::root() == Some((root.0,root.1)))
    }
    pub(crate) fn bind_root(&mut self, owner: &mut Session, id: i32) -> bool {
        let Some((root,generation)) = scene::root().filter(|(root,_)| *root == id) else { return false; };
        let next = Node(root,generation);
        if self.root == Some(next) { return true; }
        if self.root.is_some() { self.cancel(owner); }
        self.hud = None;
        self.swipe.reset();
        self.layers.clear();
        self.root = Some(next);
        self.ensure_hud(owner);
        true
    }
    fn ensure_hud(&mut self, owner: &Session) {
        if !self.root_live() { return; }
        if self.hud.as_ref().is_some_and(|mount|mount.node.live() && super::with(|registry|registry.hosts.contains_key(&mount.handle))) { return; }
        self.hud = None;
        let Some(layer) = self.layer(80) else { return; };
        let id = scene::create(scene::VIEW);
        let Some(node) = Node::bind(id) else { scene::destroy(id); return; };
        use scene::Prop::{Num,Str};
        let props = [("position",Str("absolute".into())),("left",Num(0.)),("top",Num(0.)),
            ("width",Num(0.)),("height",Num(0.)),("pointerEvents",Str("box-none".into()))];
        scene::set_style(id,&props.iter().map(|(key,value)|(*key,Some(value))).collect::<Vec<_>>());
        if !scene::insert(layer,id,0) { scene::destroy(id); return; }
        let (assets,home) = super::resource_roots();
        let mut host = Host::new(id,Box::new(super::desktop_hud::DesktopHud::with_displays(owner.reader(),super::services::displays::reader())),&assets,home.as_deref());
        host.bind_locale(home.as_deref().unwrap_or(""),&assets);
        host.facts(json!({}));
        let handle = super::register_host(host,Some(self.owner.clone()));
        self.hud = Some(Mount { nonce:0,handle,node });
    }
    pub(crate) fn hud_status(&self) -> Value {
        let mount = self.hud.as_ref().filter(|mount|self.root_live() && mount.node.live());
        json!({"handle":mount.map_or(0,|mount|mount.handle),"node":mount.map_or(0,|mount|mount.node.0)})
    }
    pub(crate) fn show_desktop_hud(&mut self, owner: &Session) {
        self.ensure_hud(owner);
        if let Some(mount) = &self.hud { super::with_host(mount.handle,|host|host.action(&json!({"action":["show"]}))); }
    }
    fn cancel(&mut self, owner: &mut Session) {
        let nonce = self.opening.read().as_ref().map(|opening| opening.nonce);
        if let Some(nonce) = nonce {
            if let Ok(reply) = owner.cancel_switcher(nonce) {
                owner.queue_service_effects(vec![json!({"kind":"switcher","nonce":nonce,"reply":reply})]);
            }
        }
        self.mounted = None;
        self.pending.clear();
        self.changes.take();
    }
    pub(crate) fn generation(&self) -> Option<u64> { self.root.filter(|_|self.root_live()).map(|root|root.1) }
    pub(crate) fn swipe(&mut self, owner: &mut Session, phase: i32, fingers: i32, dx: f64, dy: f64) -> bool {
        let Some(generation) = self.generation() else { self.swipe.reset(); return false; };
        if self.opening.read().is_some() { self.swipe.reset(); return true; }
        if let Some(gesture) = self.swipe.event(phase,fingers,dx,dy) {
            if matches!(gesture.direction,"left"|"right") {
                let reply = owner.desktop_service().borrow().swipe(owner,gesture.direction=="left");
                if reply["result"]["boundary"]==true { self.show_desktop_hud(owner); }
                if reply["state"].is_object() { owner.queue_service_effects(vec![json!({"kind":"windowModel","reply":reply})]); }
            }
            owner.queue_service_effects(vec![json!({"kind":"shellGesture","scene":generation,"direction":gesture.direction,"fingers":gesture.fingers,"at":super::now_ms()})]);
        }
        true
    }
    /// Layers remain scene children until the root is destroyed. They may also
    /// contain React portals; tearing down a service must never delete siblings.
    pub(crate) fn layer(&mut self, z: i32) -> Option<i32> {
        if !self.root_live() || !(1..=110).contains(&z) { return None; }
        if let Some(node) = self.layers.get(&z).filter(|node| node.live() && scene::parent(node.0)==self.root.unwrap().0) {
            return Some(node.0);
        }
        let id = scene::create(scene::VIEW);
        let node = Node::bind(id)?;
        use scene::Prop::{Num, Str};
        let props = [("position",Str("absolute".into())),("left",Num(0.)),("top",Num(0.)),
            ("width",Str("100%".into())),("height",Str("100%".into())),
            ("pointerEvents",Str("box-none".into())),("zIndex",Num(z as f64))];
        scene::set_style(id,&props.iter().map(|(key,value)|(*key,Some(value))).collect::<Vec<_>>());
        if !scene::insert(self.root.unwrap().0,id,0) { scene::destroy(id); return None; }
        self.layers.insert(z,node);
        Some(id)
    }
    pub(crate) fn status(&self) -> Value {
        let opening = self.opening.read();
        let nonce = opening.as_ref().map(|opening| opening.nonce);
        let mounted = self.mounted.as_ref().filter(|mount|Some(mount.nonce)==nonce && mount.node.live());
        json!({"opened":opening.is_some(),"nonce":nonce,
            "handle":mounted.map_or(0,|mount|mount.handle),"node":mounted.map_or(0,|mount|mount.node.0)})
    }
    pub(crate) fn action(&mut self, owner: &mut Session, nonce: u64, action: Value) {
        if self.opening.read().as_ref().is_none_or(|opening| opening.nonce!=nonce) { return; }
        if action[0] == "cancel" { self.cancel(owner); return; }
        if let Some(mount) = self.mounted.as_ref().filter(|mount|mount.nonce==nonce && mount.node.live()) {
            super::with_host(mount.handle, |host| host.action(&json!({"action":action})));
        } else { self.pending.push_back((nonce,action)); }
    }
    pub(crate) fn key(&mut self, owner: &mut Session, code: i32, value: i32, key: &str, flags: i32, enabled: bool) -> i32 {
        if !self.root_live() { return 0; }
        self.sync_input(owner);
        let nonce = self.opening.read().as_ref().map(|opening|opening.nonce);
        use super::shell_input::{Claim,Input};
        match self.input.claim(nonce.is_some(),code,value,key,flags,enabled) {
            Claim::Pass => return 0,
            Claim::Consumed => {},
            Claim::Desktop(command) => {
                use super::shell_input::Desktop;
                self.ensure_hud(owner);
                let reply = match command {
                    Desktop::Arrow(left) => owner.desktop_service().borrow().arrow(owner,left),
                    Desktop::Add => owner.app_command(crate::winmodel::AppCommand::AddDesktopAndShow,&[]).unwrap_or(Value::Null),
                    Desktop::Close => owner.app_command(crate::winmodel::AppCommand::CloseActiveDesktop,&[]).unwrap_or(Value::Null),
                };
                if reply["result"]["boundary"]==true { self.show_desktop_hud(owner); }
                if reply["state"].is_object() { owner.queue_service_effects(vec![json!({"kind":"windowModel","reply":reply})]); }
            },
            Claim::Window(command) => {
                let reply = crate::app_control::window_command(owner,&self.commands,command,None,&Value::Null);
                if reply["state"].is_object() { owner.queue_service_effects(vec![json!({"kind":"windowModel","reply":reply})]); }
            },
            Claim::Open { hold,back } => {
                let (fullscreen,surface) = crate::app_control::switcher_source::current();
                if let Ok(reply) = owner.prepare_switcher(fullscreen,surface,hold,back) {
                    if reply.get("state").is_some() { owner.queue_service_effects(vec![json!({"kind":"windowModel","nonce":reply["result"]["nonce"],"reply":reply})]); }
                }
            },
            Claim::Active => {
                let mods = (flags & 1) | ((flags & 4) >> 1) | ((flags & 2) << 1) | ((flags & 16) >> 1);
                let reply = owner.switcher_key(nonce.unwrap(),key,mods,value!=0,flags & 1!=0);
                if reply["action"].is_array() { self.action(owner,nonce.unwrap(),reply["action"].clone()); }
            },
        }
        1 | if Input::modifier(code) { 2 } else { 0 }
    }
    pub(crate) fn sync_grabs(&self) { if self.root_live() { self.input.sync_grabs(); } }
    pub(crate) fn sync_input(&mut self, owner: &mut Session) {
        if self.input.sync() { self.cancel(owner); }
    }
    pub(crate) fn tick(&mut self, owner: &mut Session) {
        if self.root.is_none() { return; }
        self.sync_input(owner);
        if !self.root_live() {
            self.cancel(owner);
            self.hud = None;
            self.swipe.reset();
            self.root = None;
            self.layers.clear();
            return;
        }
        self.ensure_hud(owner);
        if let Some(mount) = &self.hud { super::with_host(mount.handle,|host|host.tick(super::now_ms())); }
        self.changes.take();
        let nonce = self.opening.read().as_ref().map(|opening| opening.nonce);
        if self.mounted.as_ref().is_some_and(|mount| Some(mount.nonce)!=nonce || !mount.node.live()
            || !super::with(|registry|registry.hosts.contains_key(&mount.handle))) {
            self.mounted = None;
        }
        if let Some(nonce) = nonce {
            if self.mounted.is_none() {
                let Some(layer) = self.layer(40) else { self.cancel(owner); return; };
                let id = scene::create(scene::VIEW);
                let Some(node) = Node::bind(id) else { self.cancel(owner); return; };
                use scene::Prop::{Num, Str};
                let props = [("position",Str("absolute".into())),("left",Num(0.)),("top",Num(0.)),
                    ("width",Num(0.)),("height",Num(0.)),("pointerEvents",Str("box-none".into()))];
                scene::set_style(id,&props.iter().map(|(key,value)|(*key,Some(value))).collect::<Vec<_>>());
                if !scene::insert(layer,id,0) { scene::destroy(id); self.cancel(owner); return; }
                let (assets,home) = super::resource_roots();
                let mut host = Host::new(id,Box::new(Switcher::resident(owner)),&assets,home.as_deref());
                host.bind_locale(home.as_deref().unwrap_or(""),&assets);
                host.facts(json!({"args":{"nonce":nonce},"focused":true}));
                let handle = super::register_host(host,Some(self.owner.clone()));
                self.mounted = Some(Mount { nonce,handle,node });
            }
            let handle = self.mounted.as_ref().unwrap().handle;
            while let Some((wanted,action)) = self.pending.pop_front() {
                if wanted==nonce && self.opening.read().as_ref().is_some_and(|opening|opening.nonce==nonce) {
                    super::with_host(handle,|host|host.action(&json!({"action":action})));
                }
            }
            super::with_host(handle,|host|host.tick(super::now_ms()));
            if self.opening.read().is_none() { self.mounted = None; self.changes.take(); }
        } else { self.pending.clear(); }
    }
    pub(crate) fn pending(&self) -> bool {
        self.root.is_some() && (self.changes.pending() || self.input.pending() || !self.pending.is_empty() || !self.root_live())
    }
}
