#![crate_type = "bin"]
#![feature(slicing_syntax)]
#![allow(unstable)]

extern crate arena;
extern crate getopts;
extern crate syntax;
extern crate rustc;
extern crate rustc_resolve;
extern crate rustc_typeck;
extern crate rustc_trans;
extern crate rustc_driver;

use std::io;
use syntax::ast_map;
use rustc::session::config::{self, Input};
use rustc::session::early_error;
use rustc::session::Session;
use rustc::metadata::creader;
use rustc::middle;
use rustc::middle::stability;
use rustc::middle::ty::{self, ctxt, CtxtArenas};
use rustc_trans::back::link;
use rustc_driver::driver;

mod node_id_map;
mod type_map;

fn main() {
    let args = std::os::args();

    let optgroups = &[
        // rustc
        getopts::optmulti("", "cfg", "Configure the compilation environment", "SPEC"),
        getopts::optmulti("L", "",   "Add a directory to the library search path", "PATH"),
        getopts::optopt("", "sysroot", "Override the system root", "PATH"),

        getopts::optmulti("", "node-id-map", "output path of NodeId map", "PATH"),
        getopts::optmulti("", "type-map", "output path of NodeId-to-Type map", "PATH"),
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
            println!("{}", getopts::usage("typo [OPTIONS] [INPUT]", &*optgroups));
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

    let node_id_map_path = matches.opt_str("node-id-map").and_then(|s| Some(Path::new(s)));
    let type_map_path = matches.opt_str("type-map").and_then(|s| Some(Path::new(s)));

    let descriptions = syntax::diagnostics::registry::Registry::new(&[]);
    let sess = rustc::session::build_session(sopts, input_file_path, descriptions);
    let cfg = config::build_configuration(&sess);
    let krate = driver::phase_1_parse_input(&sess, cfg, &input);

    let id = link::find_crate_name(Some(&sess), krate.attrs.as_slice(), &input);
    let expanded_crate = driver::phase_2_configure_and_expand(&sess, krate, &*id, None);
    let expanded_crate = expanded_crate.expect("phase 2 failed");

    let mut forest = ast_map::Forest::new(expanded_crate);
    let ast_map = driver::assign_node_ids_and_map(&sess, &mut forest);
    let arena = CtxtArenas::new();
    let ty_cx = phase_3_run_analysis_passes(sess, ast_map, &arena, id);
    let krate = ty_cx.map.krate();

    if let Some(path) = node_id_map_path {
        let mut f = io::File::create(&path).unwrap();
        node_id_map::write_node_id_dic(&mut f, ty_cx.sess.codemap(), krate).unwrap();
    }

    if let Some(path) = type_map_path {
        let mut f = io::File::create(&path).unwrap();
        type_map::write_type_map(&mut f, &ty_cx).unwrap();
    }
}

fn phase_3_run_analysis_passes<'tcx>(sess: Session,
                                     ast_map: ast_map::Map<'tcx>,
                                     arena: &'tcx CtxtArenas<'tcx>,
                                     _name: String) -> ty::ctxt<'tcx> {
    let krate = ast_map.krate();

     creader::CrateReader::new(&sess).read_crates(krate);

    let lang_items = middle::lang_items::collect_language_items(krate, &sess);

    let rustc_resolve::CrateMap {
        def_map,
        freevars,
        capture_mode_map,
        export_map: _,
        trait_map,
        external_exports: _,
        last_private_map: _,
        glob_map: _,
    } = rustc_resolve::resolve_crate(&sess,
                                     &ast_map,
                                     &lang_items,
                                     krate,
                                     rustc_resolve::MakeGlobMap::No);

    // Discard MTWT tables that aren't required past resolution.
    syntax::ext::mtwt::clear_tables();

    let named_region_map = middle::resolve_lifetime::krate(&sess, krate, &def_map);

    let region_map = middle::region::resolve_crate(&sess, krate);

     middle::check_loop::check_crate(&sess, krate);

    let stability_index = stability::Index::build(krate);

    let ty_cx = ty::mk_ctxt(sess,
                            arena,
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
