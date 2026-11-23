//! Recognition of a system swipe belongs to the session, not an event listener.
#[derive(Default)]
pub(super) struct Swipe { pending: Option<(f64,f64,i32)> }
pub(super) struct Recognized { pub direction: &'static str, pub fingers: i32 }
impl Swipe {
    pub(super) fn reset(&mut self) { self.pending = None; }
    pub(super) fn event(&mut self, phase: i32, fingers: i32, dx: f64, dy: f64) -> Option<Recognized> {
        match phase {
            1 => { self.pending = matches!(fingers,3|4).then_some((0.,0.,fingers)); },
            2 => {
                if let Some((x,y,_)) = &mut self.pending {
                    *x += dx; *y += dy;
                    if !x.is_finite() || !y.is_finite() { self.pending = None; }
                }
            },
            3 => {
                let (x,y,fingers) = self.pending.take()?;
                if x.abs().max(y.abs()) < 60. { return None; }
                let direction = if x.abs()>y.abs() { if x<0. { "left" } else { "right" } }
                    else if y<0. { "up" } else { "down" };
                return Some(Recognized { direction,fingers });
            },
            _ => self.reset(),
        }
        None
    }
}
