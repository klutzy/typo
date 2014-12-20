use std::io::IoResult;
use rustc::middle::ty::{mod, ctxt};
use rustc::util::ppaux;

pub fn write_type_map<W: Writer>(w: &mut W,
                                 ty_cx: &ty::ctxt) -> IoResult<()> {
    let node_map = ty_cx.node_types.borrow();
    for (nid, &ty) in node_map.iter() {
        let ty = ppaux::ty_to_string(ty_cx, ty);
        let line = format!("{}\t{}", nid, ty);
        try!(w.write_line(&*line));
    }
    Ok(())
}
