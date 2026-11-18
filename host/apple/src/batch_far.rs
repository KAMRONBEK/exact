//! PROTO (rule 4): the far pass's ops (`Host::emit_far`), the frames a route's boxes take as UIKit moves its top edge.
use super::Batch;
use std::fmt::Write as _;

impl Batch {
    /// PROTO (rule 4): `{"op":"far","id":…,"x":…,"y":…,"w":…,"h":…,"property":"<route> <x y w h now>[; <t x y w h>]…"}`,
    /// the node's frame with its route's top edge at the block's top, its frame now, and its
    /// frame at each knot between, `t` of the way from now to the block's top.
    pub fn far(
        &mut self,
        id: u32,
        route: u32,
        now: [f32; 4],
        [x, y, w, h]: [f32; 4],
        knots: &[(f32, [f32; 4])],
    ) {
        let mut s = String::with_capacity(80);
        let _ = write!(s, "{{\"op\":\"far\",\"id\":{id},\"x\":");
        crate::style::push_num(&mut s, x);
        s.push_str(",\"y\":");
        crate::style::push_num(&mut s, y);
        s.push_str(",\"w\":");
        crate::style::push_num(&mut s, w);
        s.push_str(",\"h\":");
        crate::style::push_num(&mut s, h);
        let _ = write!(
            s,
            ",\"property\":\"{route} {} {} {} {}",
            now[0], now[1], now[2], now[3]
        );
        for (t, k) in knots {
            let _ = write!(s, "; {t} {} {} {} {}", k[0], k[1], k[2], k[3]);
        }
        s.push_str("\"}");
        self.ops.push(s);
    }

    /// PROTO (rule 4): `route`'s far frames are replaced by the `far` ops that follow; the far
    /// pass took `micros` and `trials` trial layouts.
    pub fn far_reset(
        &mut self,
        route: u32,
        micros: u64,
        trials: u32,
        [copy, lay, collect]: [u64; 3],
    ) {
        self.ops.push(format!(
            "{{\"op\":\"far\",\"id\":{route},\"property\":\"reset {micros} {trials} copy {copy} layout {lay} collect {collect}\"}}"
        ));
    }
}
