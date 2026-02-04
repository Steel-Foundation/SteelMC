use std::ops::Range;

pub struct Selection {
    anchor: usize,
    active: usize,
}

impl Selection {
    pub const fn new() -> Self {
        Selection {
            anchor: 0,
            active: 0,
        }
    }

    pub const fn is_active(&self) -> bool {
        self.anchor != self.active
    }

    pub const fn get_range(&self) -> Range<usize> {
        if self.anchor <= self.active {
            self.anchor..self.active
        } else {
            self.active..self.anchor
        }
    }

    pub const fn clear(&mut self) {
        self.anchor = 0;
        self.active = 0;
    }

    pub const fn set(&mut self, anchor: usize, active: usize) {
        self.anchor = anchor;
        self.active = active;
    }

    pub const fn extend(&mut self, new_active: usize) {
        self.active = new_active;
    }

    pub const fn start_at(&mut self, pos: usize) {
        self.anchor = pos;
        self.active = pos;
    }
}
