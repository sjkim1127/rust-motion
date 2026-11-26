//! Layer lock flags.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locks {
    pub transparency: bool,
    pub pixels: bool,
    pub position: bool,
    pub artboard: bool,
    pub all: bool,
}

impl Locks {
    /// Set the lock a command names (`transparency`, `pixels`, `position`, `artboard`, `all`);
    /// false for an unknown name. Lock Image Pixels includes Lock Transparent Pixels, so, as in
    /// Photoshop, locking the pixels clears the transparency lock, and that one stays off while the
    /// pixels are locked (#4892).
    pub fn set(&mut self, key: &str, on: bool) -> bool {
        match key {
            "transparency" => self.transparency = on && !self.pixels,
            "pixels" => {
                self.pixels = on;
                if on {
                    self.transparency = false;
                }
            }
            "position" => self.position = on,
            "artboard" => self.artboard = on,
            "all" => self.all = on,
            _ => return false,
        }
        true
    }

    /// Whether any lock is set: Photoshop then shows the lock icon on the layer's row.
    pub fn any(self) -> bool {
        self.transparency || self.pixels || self.position || self.artboard || self.all
    }

    pub fn union(self, o: Locks) -> Locks {
        Locks {
            transparency: self.transparency || o.transparency,
            pixels: self.pixels || o.pixels,
            position: self.position || o.position,
            artboard: self.artboard || o.artboard,
            all: self.all || o.all,
        }
    }
}
