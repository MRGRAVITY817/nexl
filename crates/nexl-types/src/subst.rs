//! Type substitution: mapping type variables to concrete types.

use std::collections::{HashMap, HashSet};

use crate::types::{EffectRow, Type, TypeVar};

/// A substitution maps type variables to types.
///
/// Applying a substitution walks a type recursively, replacing each `Var`
/// that appears in the map with its target (and recursing into the target
/// in case of transitive substitutions).
#[derive(Debug, Clone, Default)]
pub struct Subst {
    map: HashMap<TypeVar, Type>,
    effect_rows: HashMap<String, EffectRow>,
    next_effect_var: u32,
}

impl Subst {
    /// An empty substitution (identity).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Bind `tv` to `ty` in this substitution.
    pub fn insert(&mut self, tv: TypeVar, ty: Type) {
        self.map.insert(tv, ty);
    }

    /// Bind `row_var` to an effect row in this substitution.
    pub fn insert_effect_row(&mut self, row_var: String, row: EffectRow) {
        self.effect_rows.insert(row_var, row);
    }

    /// Allocate a fresh effect row variable name.
    pub fn fresh_effect_var(&mut self) -> String {
        let name = format!("_e{}", self.next_effect_var);
        self.next_effect_var += 1;
        name
    }

    /// Apply this substitution to an effect row, recursively replacing tail vars.
    pub fn apply_effect_row(&self, row: &EffectRow) -> EffectRow {
        let mut effects = row.effects.clone();
        let mut tail = row.tail.clone();
        let mut seen: HashSet<String> = HashSet::new();

        while let Some(var) = tail.clone() {
            if !seen.insert(var.clone()) {
                break;
            }
            let Some(bound) = self.effect_rows.get(&var) else {
                break;
            };
            effects.extend(bound.effects.iter().cloned());
            tail = bound.tail.clone();
        }

        EffectRow::new(effects, tail)
    }

    /// Apply this substitution to a type, recursively replacing variables.
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(tv) => {
                let mut current = *tv;
                let mut seen = Vec::new();
                loop {
                    if seen.contains(&current) {
                        return Type::Var(*tv);
                    }
                    seen.push(current);
                    let Some(replacement) = self.map.get(&current) else {
                        return Type::Var(current);
                    };
                    match replacement {
                        Type::Var(next) => current = *next,
                        _ => return self.apply(replacement),
                    }
                }
            }
            Type::Fn {
                params,
                ret,
                effects,
            } => Type::Fn {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
                effects: self.apply_effect_row(effects),
            },
            Type::Adt { name, args } => Type::Adt {
                name: name.clone(),
                args: args.iter().map(|a| self.apply(a)).collect(),
            },
            Type::Record { name, fields } => Type::Record {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(field, ty)| (field.clone(), self.apply(ty)))
                    .collect(),
            },
            Type::Tuple(items) => Type::Tuple(items.iter().map(|item| self.apply(item)).collect()),
            Type::Vec(elem) => Type::Vec(Box::new(self.apply(elem))),
            Type::Map { key, val } => Type::Map {
                key: Box::new(self.apply(key)),
                val: Box::new(self.apply(val)),
            },
            Type::Set(elem) => Type::Set(Box::new(self.apply(elem))),
            // Primitives and fixed-width types are unchanged.
            _ => ty.clone(),
        }
    }

    /// Compose `other` into `self`: applying the result is equivalent to
    /// first applying `self`, then `other`.
    pub fn compose(&mut self, other: &Subst) {
        // Apply `other` to every existing target in self.
        for ty in self.map.values_mut() {
            *ty = other.apply(ty);
        }
        for row in self.effect_rows.values_mut() {
            *row = other.apply_effect_row(row);
        }
        // Add any bindings from `other` that aren't already in `self`.
        for (&tv, ty) in &other.map {
            self.map.entry(tv).or_insert_with(|| ty.clone());
        }
        for (row_var, row) in &other.effect_rows {
            self.effect_rows
                .entry(row_var.clone())
                .or_insert_with(|| row.clone());
        }
    }
}
