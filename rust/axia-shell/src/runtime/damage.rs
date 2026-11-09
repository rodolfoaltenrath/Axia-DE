use super::types::{DamageList, Rect, Ui};

impl Rect {
    pub fn area(self) -> f32 {
        if self.x1 > self.x0 && self.y1 > self.y0 {
            (self.x1 - self.x0) * (self.y1 - self.y0)
        } else {
            0.
        }
    }
    fn meets(self, b: Self) -> bool {
        self.x0 < b.x1 && b.x0 < self.x1 && self.y0 < b.y1 && b.y0 < self.y1
    }
    fn join(self, b: Self) -> Self {
        Self {
            x0: self.x0.min(b.x0),
            y0: self.y0.min(b.y0),
            x1: self.x1.max(b.x1),
            y1: self.y1.max(b.y1),
        }
    }
    fn empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}
impl DamageList {
    pub fn add(&mut self, mut b: Rect) {
        if b.empty() {
            return;
        }
        loop {
            let mut i = 0;
            while i < self.n as usize {
                if self.r[i].meets(b) {
                    b = b.join(self.r[i]);
                    self.n -= 1;
                    self.r[i] = self.r[self.n as usize];
                    i = 0;
                } else {
                    i += 1;
                }
            }
            if self.n < 16 {
                break;
            }
            let (mut bi, mut bj, mut best) = (0, 1, 1e30_f32);
            for i in 0..16 {
                for j in i + 1..16 {
                    let cost =
                        self.r[i].join(self.r[j]).area() - self.r[i].area() - self.r[j].area();
                    if cost < best {
                        (bi, bj, best) = (i, j, cost);
                    }
                }
            }
            let merged = self.r[bi].join(self.r[bj]);
            self.n -= 1;
            self.r[bj] = self.r[self.n as usize];
            self.n -= 1;
            self.r[bi] = self.r[self.n as usize];
            // Merge that pair before the new rectangle, preserving damage order.
            self.add(merged);
        }
        self.r[self.n as usize] = b;
        self.n += 1;
    }
}
impl Ui {
    pub fn add_damage(&mut self, b: Rect) {
        if b.empty() {
            return;
        }
        if self.damage != self.dlast {
            self.dlist.n = 0;
            self.dlist.add(self.damage);
        }
        self.damage = if self.damage.empty() {
            b
        } else {
            self.damage.join(b)
        };
        self.dlist.add(b);
        self.dlast = self.damage;
    }
}
