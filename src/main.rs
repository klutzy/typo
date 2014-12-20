#![crate_type = "bin"]
#![feature(slicing_syntax)]

extern crate arena;
extern crate getopts;
extern crate syntax;
extern crate rustc;
extern crate rustc_typeck;
extern crate rustc_trans;
extern crate rustc_driver;

use std::cell::RefCell;
use std::io;
use arena::TypedArena;
use syntax::ast_map;
use rustc::session::config::{mod, Input};
use rustc::session::early_error;
use rustc::session::Session;
use rustc::metadata::creader;
use rustc::middle;
use rustc::middle::stability;
use rustc::middle::ty::{mod, ctxt};
use rustc_trans::back::link;
use rustc_driver::driver;

mod node_id_map;
mod type_map;
mod tags;

fn main() {
    let args = std::os::args();

    let optgroups = &[
        // rustc
        getopts::optmulti("", "cfg", "Configure the compilation environment", "SPEC"),
        getopts::optmulti("L", "",   "Add a directory to the library search path", "PATH"),
        getopts::optopt("", "sysroot", "Override the system root", "PATH"),

        // typo
        getopts::optmulti("", "tags", "output path of ctags", "PATH"),
        getopts::optmulti("", "node-id-map", "output path of NodeId map", "PATH"),
        getopts::optmulti("", "type-map", "output path of NodeId-to-Type map", "PATH"),
    ];

    let matches = match getopts::getopts(args[1..], &*optgroups) {
        Ok(m) => m,
        Err(f) => early_error(&*f.to_string()),
    };

    let sopts = {
        let mut sopts = config::basic_options();
        sopts.cfg = config::parse_cfgspecs(matches.opt_strs("cfg"));
        let addl_lib_search_paths = matches.opt_strs("L").iter().map(|s| {
            Path::new(s.as_slice())
        }).collect();
        sopts.addl_lib_search_paths = RefCell::new(addl_lib_search_paths);
        sopts.maybe_sysroot = matches.opt_str("sysroot").map(|m| Path::new(m));
        sopts
    };

    let (input, input_file_path) = match matches.free.len() {
        0u => {
            println!("{}", getopts::usage("typo [OPTIONS] [INPUT]", &*optgroups));
            early_error("no input filename given");
        }
        1u => {
            let ifile = matches.free[0].as_slice();
            if ifile == "-" {
                let contents = io::stdin().read_to_end().unwrap();
                let src = String::from_utf8(contents).unwrap();
                (Input::Str(src), None)
            } else {
                (Input::File(Path::new(ifile)), Some(Path::new(ifile)))
            }
        }
        // TODO
        _ => early_error("multiple crates not supported yet")
    };

    let tag_path = matches.opt_str("tags").and_then(|s| Some(Path::new(s)));
    let node_id_map_path = matches.opt_str("node-id-map").and_then(|s| Some(Path::new(s)));
    let type_map_path = matches.opt_str("type-map").and_then(|s| Some(Path::new(s)));

    let descriptions = syntax::diagnostics::registry::Registry::new(&[]);
    let sess = rustc::session::build_session(sopts, input_file_path, descriptions);
    let cfg = config::build_configuration(&sess);
    let krate = driver::phase_1_parse_input(&sess, cfg, &input);

    let tag_file = tag_path.and_then(|path| {
        // TODO: do not erase original tags if made by other program
        let mut f = io::File::create(&path);
        tags::write_header(&mut f).unwrap();
        tags::write_macros(&mut f, sess.codemap(), &krate).unwrap();
        Some(f)
    });

    let id = link::find_crate_name(Some(&sess), krate.attrs.as_slice(), &input);
    let expanded_crate = driver::phase_2_configure_and_expand(&sess, krate, &*id, None);
    let expanded_crate = expanded_crate.expect("phase 2 failed");

    if let Some(mut f) = tag_file {
        tags::write_defs(&mut f, sess.codemap(), &expanded_crate).unwrap();
    }

    if type_map_path.is_none() {
        return;
    }

    let mut forest = ast_map::Forest::new(expanded_crate);
    let ast_map = driver::assign_node_ids_and_map(&sess, &mut forest);
    let type_arena = TypedArena::new();
    let ty_cx = phase_3_run_analysis_passes(sess, ast_map, &type_arena, id);
    let krate = ty_cx.map.krate();

    if let Some(path) = node_id_map_path {
        let mut f = io::File::create(&path);
        node_id_map::write_node_id_dic(&mut f, ty_cx.sess.codemap(), krate).unwrap();
    }

    if let Some(path) = type_map_path {
        let mut f = io::File::create(&path);
        type_map::write_type_map(&mut f, &ty_cx).unwrap();
    }
}

fn phase_3_run_analysis_passes<'tcx>(sess: Session,
                                     ast_map: ast_map::Map<'tcx>,
                                     type_arena: &'tcx TypedArena<ty::TyS<'tcx>>,
                                     _name: String) -> ty::ctxt<'tcx> {
    let krate = ast_map.krate();

     creader::read_crates(&sess, krate);

    let lang_items = middle::lang_items::collect_language_items(krate, &sess);

    let middle::resolve::CrateMap {
        def_map,
        freevars,
        capture_mode_map,
        exp_map2: _,
        trait_map,
        external_exports: _,
        last_private_map: _
    } = middle::resolve::resolve_crate(&sess, &lang_items, krate);

    // Discard MTWT tables that aren't required past resolution.
    syntax::ext::mtwt::clear_tables();

    let named_region_map = middle::resolve_lifetime::krate(&sess, krate, &def_map);

    let region_map = middle::region::resolve_crate(&sess, krate);

     middle::check_loop::check_crate(&sess, krate);

    let stability_index = stability::Index::build(krate);

    let ty_cx = ty::mk_ctxt(sess,
                            type_arena,
                            def_map,
                            named_region_map,
                            ast_map,
                            freevars,
                            capture_mode_map,
                            region_map,
                            lang_items,
                            stability_index);

    rustc_typeck::check_crate(&ty_cx, trait_map);

    // skip other steps.
    // also do not abort even if type check failed.

    ty_cx
}
