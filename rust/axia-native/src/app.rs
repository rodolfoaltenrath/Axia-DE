use serde_json::Value;
/// What a native view needs from an app: the request interface of compiled apps
/// and the RGBA pixels of the rasters its trees name.
pub trait NativeApp {
    /// Migrated session models share an immutable native appearance plan.
    /// Legacy/isolated requests still accept their own context facts.
    fn uses_native_appearance(&self) -> bool { false }
    fn set_native_appearance(&mut self, _theme: std::rc::Rc<Value>) {}
    /// A native state subscription has an unread revision. The host schedules
    /// an immediate sync; the app consumes the invalidation during that sync.
    fn invalidated(&self) -> bool { false }
    /// Resource roots supplied by the host, including user language packs.
    fn resources(&mut self, _assets: &str, _home: Option<&str>) {}
    /// An app may vary its refresh cadence with its current page and state.
    fn refresh_ms(&self) -> Option<f64> {
        None
    }
    /// Active media may need timely control even when its window loses focus.
    fn background_refresh_ms(&self) -> Option<f64> {
        None
    }
    fn request(&mut self, input: &Value) -> Value;
    /// Whether the scene model consumes profile preferences. Only these views
    /// subscribe; presentation-only views do not wake for unrelated edits.
    fn uses_preferences(&self) -> bool { false }
    /// Session sync borrows the authoritative preferences, with no JSON snapshot.
    /// The read is scoped to this call; persist edits through deferred effects.
    /// Isolated/legacy callers continue to use request(input).
    fn sync_with_preferences(&mut self, input: &Value, _preferences: &Value) -> Value {
        self.request(input)
    }
    /// Full specification behind a revisioned marker, for remount/context recovery.
    fn retained_tree(&self, _key: &str) -> Option<&Value> {
        None
    }
    /// Custom native editors share the host's text-focus and paste routing.
    fn text_input(&self) -> bool {
        false
    }
    fn paste(&mut self, _text: &str) -> Option<Value> {
        None
    }
    fn blur(&mut self) {}
    fn pixels(&self, _id: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
    /// Transfer an already rendered opaque premultiplied ARGB frame without a UI-thread
    /// copy or conversion. The scene becomes its sole owner.
    fn take_raster(&mut self, _id: &str) -> Option<(usize, usize, Vec<u32>)> {
        None
    }
    /// Optional window-level keyboard input (e.g. Calculator). Fields keep
    /// their existing editing path; None leaves the key with the shell.
    fn key(&mut self, _key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        None
    }
}
