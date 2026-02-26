//! Built-in effect definitions for the Nexl language.
//!
//! Contains the [`BuiltinEffects`] registry which holds [`EffectDef`]s for
//! effects that are part of the language core (spec §10.2–§10.3):
//! - `Concurrent` — structured concurrency (fork/join/race)
//! - `Chan` — channel-based communication

use std::collections::HashMap;

use nexl_types::{EffectDef, EffectOpDef, EffectRow, Type, TypeVarSupply};

/// Registry of built-in effect definitions.
#[derive(Debug, Clone)]
pub struct BuiltinEffects {
    effects: HashMap<String, EffectDef>,
}

impl BuiltinEffects {
    /// Build the registry with all built-in effects.
    pub fn new() -> Self {
        let mut effects = HashMap::new();
        let concurrent = build_concurrent_effect();
        effects.insert(concurrent.name.clone(), concurrent);
        let chan = build_chan_effect();
        effects.insert(chan.name.clone(), chan);
        Self { effects }
    }

    /// Look up a built-in effect by name.
    pub fn get(&self, name: &str) -> Option<&EffectDef> {
        self.effects.get(name)
    }
}

impl Default for BuiltinEffects {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// Build the `Concurrent` effect (spec §10.2).
///
/// ```text
/// (defeffect Concurrent
///   (fork : (Fn [(Fn [] -> a ! [e])] -> (Task a)))
///   (join : (Fn [(Task a)] -> a))
///   (race : (Fn [(Vec (Task a))] -> a)))
/// ```
fn build_concurrent_effect() -> EffectDef {
    let mut supply = TypeVarSupply::new();
    let a = supply.fresh(); // t0
    let e_row = EffectRow::new(Vec::new(), Some("e".to_string()));

    // (Fn [] -> a ! [e])
    let thunk = Type::Fn {
        params: vec![],
        ret: Box::new(Type::Var(a)),
        effects: e_row,
    };

    // (Task a)
    let task_a = Type::Adt {
        name: "Task".to_string(),
        args: vec![Type::Var(a)],
    };

    // fork : (Fn [(Fn [] -> a ! [e])] -> (Task a))
    let fork = EffectOpDef {
        name: "fork".to_string(),
        signature: Type::Fn {
            params: vec![thunk],
            ret: Box::new(task_a.clone()),
            effects: EffectRow::empty(),
        },
    };

    // join : (Fn [(Task a)] -> a)
    let join = EffectOpDef {
        name: "join".to_string(),
        signature: Type::Fn {
            params: vec![task_a.clone()],
            ret: Box::new(Type::Var(a)),
            effects: EffectRow::empty(),
        },
    };

    // race : (Fn [(Vec (Task a))] -> a)
    let race = EffectOpDef {
        name: "race".to_string(),
        signature: Type::Fn {
            params: vec![Type::Vec(Box::new(task_a))],
            ret: Box::new(Type::Var(a)),
            effects: EffectRow::empty(),
        },
    };

    EffectDef {
        name: "Concurrent".to_string(),
        params: vec![],
        operations: vec![fork, join, race],
    }
}

/// Build the `Chan` effect (spec §10.3).
///
/// ```text
/// (defeffect Chan
///   (make-channel : (Fn [Int] -> (Channel a)))
///   (send!        : (Fn [(Channel a) a] -> Unit))
///   (recv!        : (Fn [(Channel a)] -> a))
///   (close!       : (Fn [(Channel a)] -> Unit)))
/// ```
fn build_chan_effect() -> EffectDef {
    let mut supply = TypeVarSupply::new();
    let a = supply.fresh(); // t0

    // (Channel a)
    let channel_a = Type::Adt {
        name: "Channel".to_string(),
        args: vec![Type::Var(a)],
    };

    // make-channel : (Fn [Int] -> (Channel a))
    let make_channel = EffectOpDef {
        name: "make-channel".to_string(),
        signature: Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(channel_a.clone()),
            effects: EffectRow::empty(),
        },
    };

    // send! : (Fn [(Channel a) a] -> Unit)
    let send = EffectOpDef {
        name: "send!".to_string(),
        signature: Type::Fn {
            params: vec![channel_a.clone(), Type::Var(a)],
            ret: Box::new(Type::Unit),
            effects: EffectRow::empty(),
        },
    };

    // recv! : (Fn [(Channel a)] -> a)
    let recv = EffectOpDef {
        name: "recv!".to_string(),
        signature: Type::Fn {
            params: vec![channel_a.clone()],
            ret: Box::new(Type::Var(a)),
            effects: EffectRow::empty(),
        },
    };

    // close! : (Fn [(Channel a)] -> Unit)
    let close = EffectOpDef {
        name: "close!".to_string(),
        signature: Type::Fn {
            params: vec![channel_a],
            ret: Box::new(Type::Unit),
            effects: EffectRow::empty(),
        },
    };

    EffectDef {
        name: "Chan".to_string(),
        params: vec![],
        operations: vec![make_channel, send, recv, close],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Test 2 --
    #[test]
    fn test_builtin_registry_has_concurrent() {
        let registry = BuiltinEffects::new();
        let concurrent = registry.get("Concurrent");
        assert!(concurrent.is_some(), "Concurrent effect must be registered");
        assert_eq!(concurrent.unwrap().name, "Concurrent");
    }

    // -- Test 3 --
    #[test]
    fn test_builtin_concurrent_has_fork_join_race() {
        let registry = BuiltinEffects::new();
        let concurrent = registry.get("Concurrent").unwrap();
        assert_eq!(concurrent.operations.len(), 3);

        let op_names: Vec<&str> = concurrent.operations.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(op_names, vec!["fork", "join", "race"]);

        // fork takes a thunk and returns (Task a)
        match &concurrent.operations[0].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1, "fork takes one param (a thunk)");
                // Return type should be (Task a) = Adt { name: "Task", .. }
                match ret.as_ref() {
                    nexl_types::Type::Adt { name, args } => {
                        assert_eq!(name, "Task");
                        assert_eq!(args.len(), 1);
                    }
                    other => panic!("fork return type should be Adt(Task), got {other:?}"),
                }
            }
            other => panic!("fork signature should be Fn, got {other:?}"),
        }

        // join takes (Task a) and returns a
        match &concurrent.operations[1].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1, "join takes one param (a Task)");
                match &params[0] {
                    nexl_types::Type::Adt { name, .. } => assert_eq!(name, "Task"),
                    other => panic!("join param should be Adt(Task), got {other:?}"),
                }
                // Return type should be a type variable
                assert!(matches!(ret.as_ref(), nexl_types::Type::Var(_)));
            }
            other => panic!("join signature should be Fn, got {other:?}"),
        }

        // race takes (Vec (Task a)) and returns a
        match &concurrent.operations[2].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1, "race takes one param (Vec of Tasks)");
                match &params[0] {
                    nexl_types::Type::Vec(inner) => match inner.as_ref() {
                        nexl_types::Type::Adt { name, .. } => assert_eq!(name, "Task"),
                        other => panic!("race param inner should be Adt(Task), got {other:?}"),
                    },
                    other => panic!("race param should be Vec, got {other:?}"),
                }
                assert!(matches!(ret.as_ref(), nexl_types::Type::Var(_)));
            }
            other => panic!("race signature should be Fn, got {other:?}"),
        }
    }

    // -- Test 4 --
    #[test]
    fn test_builtin_registry_has_chan() {
        let registry = BuiltinEffects::new();
        let chan = registry.get("Chan");
        assert!(chan.is_some(), "Chan effect must be registered");
        assert_eq!(chan.unwrap().name, "Chan");
    }

    // -- Test 5 --
    #[test]
    fn test_builtin_chan_has_operations() {
        let registry = BuiltinEffects::new();
        let chan = registry.get("Chan").unwrap();
        assert_eq!(chan.operations.len(), 4);

        let op_names: Vec<&str> = chan.operations.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(op_names, vec!["make-channel", "send!", "recv!", "close!"]);

        // make-channel takes Int, returns (Channel a)
        match &chan.operations[0].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], nexl_types::Type::Int);
                match ret.as_ref() {
                    nexl_types::Type::Adt { name, args } => {
                        assert_eq!(name, "Channel");
                        assert_eq!(args.len(), 1);
                    }
                    other => panic!("make-channel ret should be Adt(Channel), got {other:?}"),
                }
            }
            other => panic!("make-channel sig should be Fn, got {other:?}"),
        }

        // send! takes (Channel a) and a, returns Unit
        match &chan.operations[1].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 2);
                match &params[0] {
                    nexl_types::Type::Adt { name, .. } => assert_eq!(name, "Channel"),
                    other => panic!("send! first param should be Channel, got {other:?}"),
                }
                assert!(matches!(&params[1], nexl_types::Type::Var(_)));
                assert_eq!(ret.as_ref(), &nexl_types::Type::Unit);
            }
            other => panic!("send! sig should be Fn, got {other:?}"),
        }

        // recv! takes (Channel a), returns a
        match &chan.operations[2].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                match &params[0] {
                    nexl_types::Type::Adt { name, .. } => assert_eq!(name, "Channel"),
                    other => panic!("recv! param should be Channel, got {other:?}"),
                }
                assert!(matches!(ret.as_ref(), nexl_types::Type::Var(_)));
            }
            other => panic!("recv! sig should be Fn, got {other:?}"),
        }

        // close! takes (Channel a), returns Unit
        match &chan.operations[3].signature {
            nexl_types::Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                match &params[0] {
                    nexl_types::Type::Adt { name, .. } => assert_eq!(name, "Channel"),
                    other => panic!("close! param should be Channel, got {other:?}"),
                }
                assert_eq!(ret.as_ref(), &nexl_types::Type::Unit);
            }
            other => panic!("close! sig should be Fn, got {other:?}"),
        }
    }

    // -- Test 6 --
    #[test]
    fn test_builtin_registry_lookup_missing() {
        let registry = BuiltinEffects::new();
        assert!(registry.get("NonExistent").is_none());
        assert!(registry.get("").is_none());
        assert!(registry.get("concurrent").is_none(), "lookup is case-sensitive");
    }
}
