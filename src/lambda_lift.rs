use {
    crate::{
        core::{
            Expr::*,
            *,
        },
        renamer::{
            typ::*,
            NameSupply,
        },
    },
    std::collections::{
        hash_map::Entry,
        HashMap,
    },
};

pub type TypeAndStr = Id;

pub fn do_lambda_lift(module: Module<TypeAndStr>) -> Module<Id> {
    lift_lambdas(abstract_module(module))
}

struct FreeVariables {
    name_supply: NameSupply,
}

fn each_pattern_variables(pattern: &Pattern<Id>, f: &mut dyn FnMut(&Name)) {
    match *pattern {
        Pattern::Identifier(ref ident) => (*f)(&ident.name),
        Pattern::Constructor(_, ref patterns) => {
            for p in patterns.iter() {
                (*f)(&p.name);
            }
        }
        _ => (),
    }
}

impl FreeVariables {
    //Walks through an expression and notes all the free variables and for each lambda, adds the
    //free variables to its arguments and performs an immediate application
    //@variables All the local variables in scope, values are how many of the name there exists
    //@free_vars The free variables for the returned expression
    fn free_variables(
        &mut self,
        variables: &mut HashMap<Name, isize>,
        free_vars: &mut HashMap<Name, TypeAndStr>,
        expr: &mut Expr<TypeAndStr>,
    ) {
        match *expr {
            Identifier(ref mut i) => {
                //If the identifier is a local, add it to the free variables
                if variables.get(&i.name).map(|x| *x > 0).unwrap_or(false) {
                    free_vars.insert(i.name.clone(), i.clone());
                }
            }
            Apply(ref mut func, ref mut arg) => {
                self.free_variables(variables, free_vars, func);
                self.free_variables(variables, free_vars, arg);
            }
            Lambda(ref mut arg, ref mut body) => {
                match variables.entry(arg.name.clone()) {
                    Entry::Vacant(entry) => {
                        entry.insert(1);
                    }
                    Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                }
                self.free_variables(variables, free_vars, body);
                *variables.get_mut(&arg.name).unwrap() -= 1;
                free_vars.remove(&arg.name); //arg was not actually a free variable
            }
            Let(ref mut bindings, ref mut expr) => {
                for bind in bindings.iter() {
                    match variables.entry(bind.name.name.clone()) {
                        Entry::Vacant(entry) => {
                            entry.insert(1);
                        }
                        Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                    }
                }
                let mut free_vars2 = HashMap::new();
                for bind in bindings.iter_mut() {
                    free_vars2.clear();
                    self.free_variables(variables, &mut free_vars2, &mut bind.expression);
                    //free_vars2 is the free variables for this binding
                    for (k, v) in free_vars2.iter() {
                        free_vars.insert(k.clone(), v.clone());
                    }
                    self.abstract_(&free_vars2, &mut bind.expression);
                }
                self.free_variables(variables, free_vars, expr);
                for bind in bindings.iter() {
                    *variables.get_mut(&bind.name.name).unwrap() -= 1;
                    free_vars.remove(&bind.name.name);
                }
            }
            Case(ref mut expr, ref mut alts) => {
                self.free_variables(variables, free_vars, expr);
                for alt in alts.iter_mut() {
                    each_pattern_variables(&alt.pattern, &mut |name| match variables
                        .entry(name.clone())
                    {
                        Entry::Vacant(entry) => {
                            entry.insert(1);
                        }
                        Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                    });
                    self.free_variables(variables, free_vars, &mut alt.expression);
                    each_pattern_variables(&alt.pattern, &mut |name| {
                        *variables.get_mut(name).unwrap() -= 1;
                        free_vars.remove(name); //arg was not actually a free variable
                    });
                }
            }
            _ => (),
        }
    }
    ///Adds the free variables, if any, to the expression
    fn abstract_(
        &mut self,
        free_vars: &HashMap<Name, TypeAndStr>,
        input_expr: &mut Expr<TypeAndStr>,
    ) {
        if !free_vars.is_empty() {
            let mut temp = Literal(LiteralData {
                typ: Type::new_var(self.name_supply.from_str("a").name),
                value: Integral(0),
            });
            ::std::mem::swap(&mut temp, input_expr);
            let mut e = {
                let mut rhs = temp;
                let mut typ = rhs.get_type().clone();
                for (_, var) in free_vars.iter() {
                    rhs = Lambda(var.clone(), rhs.into());
                    typ = function_type_(var.get_type().clone(), typ);
                }
                let id = Id::new(self.name_supply.from_str("#sc"), typ.clone(), vec![]);
                let bind = Binding {
                    name: id.clone(),
                    expression: rhs,
                };
                Let(vec![bind], Identifier(id).into())
            };
            for (_, var) in free_vars.iter() {
                e = Apply(e.into(), Identifier(var.clone()).into());
            }
            *input_expr = e
        }
    }
}

///Lifts all lambdas in the module to the top level of the program
pub fn lift_lambdas<T>(mut module: Module<T>) -> Module<T> {
    use crate::core::mutable::*;
    struct LambdaLifter<T> {
        out_lambdas: Vec<Binding<T>>,
    }
    impl<T> Visitor<T> for LambdaLifter<T> {
        fn visit_expr(&mut self, expr: &mut Expr<T>) {
            match *expr {
                Let(ref mut bindings, ref mut body) => {
                    let mut new_binds = vec![];
                    let mut bs = vec![];
                    ::std::mem::swap(&mut bs, bindings);
                    for mut bind in bs.into_iter() {
                        let is_lambda = matches!(bind.expression, Lambda(..));
                        walk_expr(self, &mut bind.expression);
                        if is_lambda {
                            self.out_lambdas.push(bind);
                        } else {
                            new_binds.push(bind);
                        }
                    }
                    *bindings = new_binds;
                    self.visit_expr(body);
                }
                _ => walk_expr(self, expr),
            }
            remove_empty_let(expr);
        }
    }
    let mut visitor = LambdaLifter {
        out_lambdas: vec![],
    };
    visitor.visit_module(&mut module);
    let mut temp = vec![];
    ::std::mem::swap(&mut temp, &mut module.bindings);
    let vec: Vec<Binding<T>> = temp
        .into_iter()
        .chain(visitor.out_lambdas.into_iter())
        .collect();
    module.bindings = vec;
    module
}
//Replaces let expressions with no binding with the expression itself
fn remove_empty_let<T>(expr: &mut Expr<T>) {
    let mut temp = unsafe { ::std::mem::MaybeUninit::zeroed().assume_init() };
    ::std::mem::swap(&mut temp, expr);
    temp = match temp {
        Let(bindings, e) => {
            if bindings.is_empty() {
                *e
            } else {
                Let(bindings, e)
            }
        }
        temp => temp,
    };
    ::std::mem::swap(&mut temp, expr);
    ::std::mem::forget(temp);
}

///Takes a module and adds all variables which are captured into a lambda to its arguments
pub fn abstract_module(mut module: Module<TypeAndStr>) -> Module<TypeAndStr> {
    use crate::core::mutable::*;
    impl Visitor<TypeAndStr> for FreeVariables {
        fn visit_binding(&mut self, bind: &mut Binding<TypeAndStr>) {
            self.free_variables(
                &mut <_>::default(),
                &mut <_>::default(),
                &mut bind.expression,
            );
        }
    }
    let mut this =
        FreeVariables {
            name_supply: NameSupply::new(),
        };
    this.visit_module(&mut module);
    module
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            core::{
                ref_::*,
                translate::translate_module,
            },
            interner::*,
            lambda_lift::*,
            parser::Parser,
            renamer::tests::rename_module,
            typecheck::TypeEnvironment,
        },
        std::collections::HashSet,
        test::Bencher,
    };

    struct CheckUniques {
        found: HashSet<Id>,
    }

    impl Visitor<Id> for CheckUniques {
        fn visit_binding(&mut self, bind: &Binding<Id>) {
            assert!(self.found.insert(bind.name.clone()));
            self.visit_expr(&bind.expression);
        }
        fn visit_expr(&mut self, expr: &Expr<Id>) {
            match expr {
                &Lambda(ref i, _) => {
                    assert!(self.found.insert(i.clone()));
                }
                _ => (),
            }
            walk_expr(self, expr)
        }
    }

    #[test]
    fn all_uniques() {
        let mut visitor =
            CheckUniques {
                found: HashSet::new(),
            };
        let mut parser = Parser::new(
            r"add x y = 2
test = 3.14
test2 x =
    let
        test = 2
        f x =
            let g y = add x (f y)
            in add x test
    in f x"
                .chars(),
        );
        let module = translate_module(rename_module(parser.module().unwrap()));
        visitor.visit_module(&module);
    }

    fn check_args(expr: &Expr<Id>, args: &[InternedStr]) -> bool {
        match expr {
            &Lambda(ref arg, ref body) => arg.name.name == args[0] && check_args(body, &args[1..]),
            _ => args.is_empty(),
        }
    }

    struct CheckAbstract {
        count: isize,
    }

    fn get_let<'a>(expr: &'a Expr<Id>, args: &mut Vec<InternedStr>) -> &'a Expr<Id> {
        match expr {
            &Apply(ref f, ref arg) => {
                match **arg {
                    Identifier(ref i) => args.push(i.name.name),
                    _ => panic!("Expected identifier as argument"),
                }
                get_let(f, args)
            }
            _ => expr,
        }
    }

    impl Visitor<Id> for CheckAbstract {
        fn visit_binding(&mut self, bind: &Binding<Id>) {
            if intern("f") == bind.name.name.name {
                let mut args = vec![];
                match get_let(&bind.expression, &mut args) {
                    &Let(ref binds, ref body) => {
                        //Push the argument of the function itself
                        args.push(intern("x"));
                        assert!(check_args(&binds[0].expression, args.as_ref()));
                        assert_eq!(Identifier(binds[0].name.clone()), **body);
                    }
                    _ => assert!(false, "Expected Let, found {:?}", bind.expression),
                }
                self.count += 1;
            } else if intern("g") == bind.name.name.name {
                let mut args = vec![];
                match get_let(&bind.expression, &mut args) {
                    &Let(ref binds, ref body) => {
                        args.push(intern("y"));
                        assert!(check_args(&binds[0].expression, args.as_ref()));
                        assert_eq!(Identifier(binds[0].name.clone()), **body);
                    }
                    _ => assert!(false, "Expected Let"),
                }
                self.count += 1;
            }
            self.visit_expr(&bind.expression);
        }
    }

    #[test]
    fn all_free_vars() {
        let mut visitor = CheckAbstract { count: 0 };
        let mut parser = Parser::new(
            r"add x y = 2
test = 3.14
test2 x =
    let
        test = 2
        f x =
            let g y = add x (f y)
            in add x test
    in f x"
                .chars(),
        );
        let mut module = rename_module(parser.module().unwrap());
        TypeEnvironment::new()
            .typecheck_module(&mut module)
            .unwrap();
        let m = translate_module(module);
        let module = abstract_module(m);
        visitor.visit_module(&module);
        assert_eq!(visitor.count, 2);
    }

    struct NoLambdas;

    impl<T> Visitor<T> for NoLambdas {
        fn visit_expr(&mut self, expr: &Expr<T>) {
            match expr {
                &Lambda(..) => assert!(false, "Found lambda in expression"),
                _ => (),
            }
            walk_expr(self, expr);
        }
    }
    #[test]
    fn no_local_lambdas() {
        fn skip_lambdas<T>(expr: &Expr<T>) -> &Expr<T> {
            match expr {
                &Lambda(_, ref body) => skip_lambdas(body),
                _ => expr,
            }
        }

        let mut visitor = NoLambdas;
        let mut parser = Parser::new(
            r"add x y = 2
test = 3.14
test2 x =
    let
        test = 2
        f x =
            let g y = add x (f y)
            in add x test
    in f x"
                .chars(),
        );
        let m = translate_module(rename_module(parser.module().unwrap()));
        let module = lift_lambdas(m);
        for bind in module.bindings.iter() {
            visitor.visit_expr(skip_lambdas(&bind.expression));
        }
    }

    #[bench]
    fn bench(b: &mut Bencher) {
        use {
            crate::typecheck::test::do_typecheck,
            std::{
                fs::File,
                io::Read,
                path::Path,
            },
        };

        let path = &Path::new("Prelude.hs");
        let mut contents = ::std::string::String::new();
        File::open(path)
            .and_then(|mut f| f.read_to_string(&mut contents))
            .unwrap();
        let module = do_typecheck(&contents);
        b.iter(|| do_lambda_lift(translate_module(module.clone())));
    }
}
