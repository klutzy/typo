use std::io::IoResult;
use syntax::ast;
use syntax::codemap::{Span, CodeMap};
use syntax::visit::{self, Visitor};

pub fn write_node_id_dic<W: Writer>(w: &mut W,
                                    cm: &CodeMap,
                                    krate: &ast::Crate) -> IoResult<()> {
    let mut grepper = NodeIdGrepper {
        map: Vec::new()
    };
    grepper.visit_mod(&krate.module, krate.span, ast::DUMMY_NODE_ID);

    for &(sp, nid) in grepper.map.iter() {
        let begin = cm.lookup_byte_offset(sp.lo);
        let end = cm.lookup_byte_offset(sp.hi);
        let filename = &*begin.fm.name;

        let line = format!("{}\t{}\t{}\t{}",
                           filename,
                           begin.pos.0,
                           end.pos.0,
                           nid);
        try!(w.write_line(&*line));
    }
    Ok(())
}

struct NodeIdGrepper {
    map: Vec<(Span, ast::NodeId)>,
}

impl NodeIdGrepper {
    fn grep_id(&mut self, id: ast::NodeId, sp: Span) {
        self.map.push((sp, id));
    }
}

impl<'a> Visitor<'a> for NodeIdGrepper {
    fn visit_pat(&mut self, p: &'a ast::Pat) {
        self.grep_id(p.id, p.span);
        visit::walk_pat(self, p);
    }

    fn visit_path(&mut self, p: &'a ast::Path, id: ast::NodeId) {
        self.grep_id(id, p.span);
        visit::walk_path(self, p);
    }

    fn visit_expr(&mut self, ex: &'a ast::Expr) {
        match ex.node {
            // Will be visited by visit_path
            ast::ExprPath(..) => (),
            _ => {
                self.grep_id(ex.id, ex.span);
            }
        }
        visit::walk_expr(self, ex);
    }

    fn visit_stmt(&mut self, st: &'a ast::Stmt) {
        match st.node {
            ast::StmtDecl(_, id) | ast::StmtExpr(_, id) | ast::StmtSemi(_, id) => {
                self.grep_id(id, st.span);
            }
            _ => {}
        }
        visit::walk_stmt(self, st);
    }
}
