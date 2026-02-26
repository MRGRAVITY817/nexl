# Crate Dependency Map

Current workspace crates and their dependencies.

## M0 Crates

```
nexl-reader
├── nexl-ast
└── nexl-errors
    └── nexl-ast
```

## M1 Crates

```
nexl-runtime

nexl-eval
└── nexl-runtime
```

## M5 Crates

```
nexl-modules
```

## M6 Crates

```
nexl-effects
├── nexl-types
├── nexl-infer
└── nexl-errors
```

## M8 Crates

```
nexl-ir
(no workspace-crate dependencies — standalone IR node types)

nexl-wasm
├── nexl-ir (M8)
└── wasm-encoder (external)

nexl-memory
└── nexl-ir (M8)

nexl-cli
├── nexl-reader (M0)
├── nexl-ir (M8)
└── nexl-wasm (M8)
```

## M10 Crates

```
nexl-macros
└── nexl-ast
```

## M13 Crates

```
nexl-native
├── nexl-ir (M8)
├── cranelift-codegen (external)
├── cranelift-frontend (external)
├── cranelift-module (external)
├── cranelift-object (external)
└── target-lexicon (external)
```

## Planned (future milestones)

```
nexl-vm (M8+)
├── nexl-ir (M8)
└── nexl-runtime (M1)

nexl-infer (M2)
├── nexl-ast
├── nexl-types
└── nexl-errors
```

New crates are added as their milestone begins. This file is updated accordingly.
