//! A redraw booked for later, one at a time: a view drawn many times before its timer fires still
//! books a single timer. This is how a ticking clock or a moving bar redraws only while it shows,
//! and why the island renders nothing at idle.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::Context;

use crate::Island;

#[derive(Default)]
pub struct Booking(Rc<Cell<bool>>);

impl Booking {
    /// After `after`, run `fire` on the island, unless a run is booked already. The booking is
    /// clear by the time `fire` runs, so it may book the next one.
    pub fn book(
        &self,
        after: Duration,
        cx: &mut Context<Island>,
        fire: impl FnOnce(&mut Island, &mut Context<Island>) + 'static,
    ) {
        if self.0.replace(true) {
            return;
        }
        let booked = Rc::clone(&self.0);
        cx.spawn(async move |island, cx| {
            cx.background_executor().timer(after).await;
            booked.set(false);
            island.update(cx, fire).ok();
        })
        .detach();
    }
}
