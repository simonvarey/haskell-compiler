#![crate_type = "bin"]
#![cfg_attr(test, feature(test))]
#[macro_use]
extern crate log;
extern crate getopts;
#[cfg(test)]
extern crate test;

#[cfg(not(test))]
use vm::execute_main_module;
#[cfg(not(test))]
use getopts::Options;

macro_rules! write_core_expr(
    ($e:expr, $f:expr, $($p:pat),*) => ({
        match $e {
            Identifier(ref s) => write!($f, "{}", *s),
            Apply(ref func, ref arg) => write!($f, "({} {})", func, *arg),
            Literal(ref l) => write!($f, "{}", *l),
            Lambda(ref arg, ref body) => write!($f, "({} -> {})", *arg, *body),
            Let(ref bindings, ref body) => {
                write!($f, "let {{\n")?;
                for bind in bindings.iter() {
                    write!($f, "; {}\n", bind)?;
                }
                write!($f, "}} in {}\n", *body)
            }
            Case(ref expr, ref alts) => {
                write!($f, "case {} of {{\n", *expr)?;
                for alt in alts.iter() {
                    write!($f, "; {}\n", alt)?;
                }
                write!($f, "}}\n")
            }
            $($p => Ok(()))*
        }
    })
);

mod types;
mod module;
mod compiler;
mod typecheck;
mod lexer;
mod parser;
mod graph;
mod vm;
mod scoped_map;
mod core;
mod lambda_lift;
mod renamer;
mod infix;
mod builtins;
mod interner;
mod deriving;
#[cfg(not(test))]
mod repl;

#[cfg(not(test))]
fn main() {
    let mut opts = Options::new();
    opts.optflag("i", "interactive", "Starts the REPL");
    opts.optflag("h", "help", "Print help");

    let matches = {
        let args: Vec<_> = std::env::args().skip(1).collect();
        opts.parse(args).unwrap()
    };

    if matches.opt_present("h") {
        println!("Usage: vm [OPTIONS|EXPRESSION] {}", opts.usage(""));
        return;
    }

    if matches.opt_present("i") {
        repl::start();
        return;
    }

    if matches.free.len() < 1 {
        println!("Usage: vm [OPTIONS|EXPRESSION] {}", opts.usage(""));
        return;
    }

    let modulename = &*matches.free[0];
    match execute_main_module(modulename.as_ref()).unwrap() {
        Some(x) => println!("{:?}", x),
        None => println!("Error running module {}", modulename)
    }
}

