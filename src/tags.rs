#![crate_type = "bin"]

#![feature(slicing_syntax)]
#![allow(unstable)]

extern crate getopts;
extern crate syntax;
extern crate rustc;
extern crate rustc_trans;
extern crate rustc_driver;

use std::io::{self, Writer, IoResult};
use syntax::ast;
use syntax::codemap::{Span, CodeMap};
use syntax::visit::{self, Visitor};
use rustc::session::config::{self, Input};
use rustc::session::early_error;
use rustc_trans::back::link;
use rustc_driver::driver;

fn main() {
    let args = std::os::args();

    let optgroups = &[
        // rustc
        getopts::optmulti("", "cfg", "Configure the compilation environment", "SPEC"),
        getopts::optmulti("L", "",   "Add a directory to the library search path", "PATH"),
        getopts::optopt("", "sysroot", "Override the system root", "PATH"),

        // typo
        getopts::optmulti("", "tags", "output path of ctags", "PATH"),
        getopts::optflag("", "tags-append", "append to existing tags"),
    ];

    let matches = match getopts::getopts(&args[1..], &*optgroups) {
        Ok(m) => m,
        Err(f) => early_error(&*f.to_string()),
    };

    let sopts = {
        let mut sopts = config::basic_options();
        sopts.cfg = config::parse_cfgspecs(matches.opt_strs("cfg"));
        for s in matches.opt_strs("L").iter() {
            sopts.search_paths.add_path(&**s);
        }
        sopts.maybe_sysroot = matches.opt_str("sysroot").map(|m| Path::new(m));
        sopts
    };

    let (input, input_file_path) = match matches.free.len() {
        0us => {
            println!("{}", getopts::usage("typo-tags [OPTIONS] [INPUT]", &*optgroups));
            early_error("no input filename given");
        }
        1us => {
            let ifile = matches.free[0].as_slice();
            if ifile == "-" {
                let contents = io::stdin().read_to_end().unwrap();
                let src = String::from_utf8(contents).unwrap();
                (Input::Str(src), None)
            } else {
                (Input::File(Path::new(ifile)), Some(Path::new(ifile)))
            }
        }
        _ => early_error("multiple input found")
    };

    let tag_path = matches.opt_str("tags").unwrap_or("tags".to_string());
    let tag_path = Path::new(tag_path);
    let tags_append = matches.opt_present("tags-append");

    let descriptions = syntax::diagnostics::registry::Registry::new(&[]);
    let sess = rustc::session::build_session(sopts, input_file_path, descriptions);
    let cfg = config::build_configuration(&sess);
    let krate = driver::phase_1_parse_input(&sess, cfg, &input);

    // TODO: do not erase original tags if made by other program
    let mut f = if tags_append {
        io::File::open_mode(&tag_path, io::Append, io::Write).unwrap()
    } else {
        let mut f = io::File::create(&tag_path).unwrap();
        write_header(&mut f).unwrap();
        f
    };

    write_macros(&mut f, sess.codemap(), &krate).unwrap();

    let id = link::find_crate_name(Some(&sess), krate.attrs.as_slice(), &input);
    let expanded_crate = driver::phase_2_configure_and_expand(&sess, krate, &*id, None);
    let expanded_crate = expanded_crate.expect("phase 2 failed");

    write_defs(&mut f, sess.codemap(), &expanded_crate).unwrap();
}

pub fn write_header<W: Writer>(writer: &mut W) -> IoResult<()> {
    let info = [
        ("TAG_FILE_FORMAT", "1"), // original
        ("TAG_FILE_SORTED", "0"), // unsorted: 0, sorted: 1
        ("TAG_PROGRAM_NAME", "typo"),
    ];
    for &(name, val) in info.iter() {
        let line = format!("!_{}\t{}", name, val);
        try!(writer.write_line(&*line));
    }

    Ok(())
}

fn write_line<W: Writer>(writer: &mut W,
                         cm: &CodeMap,
                         id: ast::Ident,
                         sp: Span) -> IoResult<()> {
    let begin = cm.lookup_byte_offset(sp.lo);
    let begin_loc = cm.lookup_char_pos(sp.lo);

    let filename = &*begin.fm.name;
    let line_number = begin_loc.line - 1;
    let line = begin.fm.get_line(line_number);

    // TODO: what if line is None?
    if let Some(line) = line {
        // <id> '\t' <filename> '\t' '/^' <line> '$/'

        try!(writer.write_str(id.as_str()));
        try!(writer.write_u8(b'\t'));
        try!(writer.write_str(filename));
        try!(writer.write_u8(b'\t'));

        try!(writer.write(b"/^"));
        for c in line.chars() {
            if c == '/' || c == '$' || c == '\\' {
                try!(writer.write_u8(b'\\'));
            }
            try!(writer.write_char(c));
        }
        try!(writer.write(b"$/"));

        try!(writer.write_u8(b'\n'));
    }

    Ok(())
}

pub fn write_macros<W: Writer>(writer: &mut W,
                               cm: &CodeMap,
                               krate: &ast::Crate) -> IoResult<()> {
    let mut grepper = MacroTagGrepper {
        macro_map: Vec::new(),
    };
    grepper.visit_mod(&krate.module, krate.span, ast::DUMMY_NODE_ID);

    for &(id, sp) in grepper.macro_map.iter() {
        try!(write_line(writer, cm, id, sp));
    }
    Ok(())
}

pub fn write_defs<W: Writer>(writer: &mut W, cm: &CodeMap, krate: &ast::Crate) -> IoResult<()> {
    let mut grepper = TagGrepper {
        map: Vec::new(),
        cm: cm,
    };
    grepper.visit_mod(&krate.module, krate.span, ast::DUMMY_NODE_ID);

    for &(id, sp) in grepper.map.iter() {
        try!(write_line(writer, cm, id, sp));
    }
    Ok(())
}

// TagGrepper is invoked after expansion, so it doesn't know any macros.
// we need to run this before expansion.
pub struct MacroTagGrepper {
    pub macro_map: Vec<(ast::Ident, Span)>,
}

impl<'a> Visitor<'a> for MacroTagGrepper {
    fn visit_item(&mut self, i: &'a ast::Item) {
        match i.node {
            ast::ItemMac(ref mac) => {
                match mac.node {
                    ast::MacInvocTT(ref p, _, _) => {
                        // currently Rust assumes `p` has only one segment.
                        if p.segments.len() == 1 {
                            let p0 = &p.segments[0];
                            if p0.identifier.as_str() == "macro_rules" {
                                self.macro_map.push((i.ident, p.span));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        visit::walk_item(self, i);
    }

    fn visit_mac(&mut self, m: &'a ast::Mac) {
        visit::walk_mac(self, m);
    }
}

pub struct TagGrepper<'b> {
    pub map: Vec<(ast::Ident, Span)>,
    cm: &'b CodeMap,
}

impl<'b> TagGrepper<'b> {
    fn grep_id(&mut self, id: ast::Ident, span: Span) {
        self.map.push((id, span));
    }
}

impl<'a, 'b> Visitor<'a> for TagGrepper<'b> {
    fn visit_foreign_item(&mut self, i: &'a ast::ForeignItem) {
        self.grep_id(i.ident, i.span);
        visit::walk_foreign_item(self, i);
    }

    fn visit_view_item(&mut self, i: &'a ast::ViewItem) {
        // `use ... as new_ident;`
        if let ast::ViewItemUse(ref vp) = i.node {
            if let ast::ViewPathSimple(renamed, ref p, _) = vp.node {
                let p = &*p.segments;
                if p.len() >= 1 && renamed.name != p[p.len() - 1].identifier.name {
                    self.grep_id(renamed, vp.span);
                }
            }
        }
        visit::walk_view_item(self, i);
    }

    fn visit_item(&mut self, i: &'a ast::Item) {
        let span = match i.node {
            ast::ItemMod(ref m) => {
                // for `mod item;`, find inner file which contains
                // actual code.
                let inner_sp = m.inner;
                let inner_begin = self.cm.lookup_byte_offset(inner_sp.lo);
                let inner_filename = &*inner_begin.fm.name;

                let outer_begin = self.cm.lookup_byte_offset(i.span.lo);
                let outer_filename = &*outer_begin.fm.name;

                if inner_filename != outer_filename {
                    inner_sp
                } else {
                    i.span
                }
            }
            _ => i.span,
        };
        self.grep_id(i.ident, span);
        visit::walk_item(self, i);
    }

    fn visit_fn(&mut self,
                fk: visit::FnKind<'a>,
                fd: &'a ast::FnDecl,
                b: &'a ast::Block,
                sp: Span,
                _: ast::NodeId) {
        match fk {
            visit::FkMethod(id, _, _) => {
                self.grep_id(id, sp);
            }
            // skip FkItemFn: checked by visit_item
            // skip FkFnBlock: no name
            _ => {}
        }
        visit::walk_fn(self, fk, fd, b, sp);
    }

    fn visit_ty_method(&mut self, t: &'a ast::TypeMethod) {
        self.grep_id(t.ident, t.span);
        visit::walk_ty_method(self, t);
    }

    fn visit_trait_item(&mut self, t: &'a ast::TraitItem) {
        match *t {
            ast::RequiredMethod(_) | ast::ProvidedMethod(_) => {
                // `visit_ty_method` and `visit_fn` will be called later.
            }
            ast::TypeTraitItem(ref at) => {
                // TODO: is this "local"? good to add entry to tags? needs more investigation.
                self.grep_id(at.ty_param.ident, at.ty_param.span);
            }
        }
        visit::walk_trait_item(self, t);
    }

    fn visit_struct_field(&mut self, s: &'a ast::StructField) {
        // skip tuple struct field
        if let ast::NamedField(ref id, _) = s.node.kind {
            self.grep_id(*id, s.span);
        }
        visit::walk_struct_field(self, s);
    }

    fn visit_variant(&mut self, v: &'a ast::Variant, g: &'a ast::Generics) {
        self.grep_id(v.node.name, v.span);
        visit::walk_variant(self, v, g);
    }
}
