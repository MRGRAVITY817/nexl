//! Type substitution: mapping type variables to concrete types.

use std::collections::HashMap;

use crate::types::{Type, TypeVar};

/// A substitution maps type variables to types.
///
/// Applying a substitution walks a type recursively, replacing each `Var`
/// that appears in the map with its target (and recursing into the target
/// in case of transitive substitutions).
#[derive(Debug, Clone, Default)]
pub struct Subst {
    map: HashMap<TypeVar, Type>,
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

    /// Apply this substitution to a type, recursively replacing variables.
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(tv) => match self.map.get(tv) {
                Some(replacement) => self.apply(replacement),
                None => ty.clone(),
            },
            Type::Fn { params, ret } => Type::Fn {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
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
        // Add any bindings from `other` that aren't already in `self`.
        for (&tv, ty) in &other.map {
            self.map.entry(tv).or_insert_with(|| ty.clone());
        }
    }
}
