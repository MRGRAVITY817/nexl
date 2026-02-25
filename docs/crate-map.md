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

## Planned (future milestones)

```
nexl-cli (M8)
├── nexl-reader (M0)
├── nexl-eval (M1, temporary)
├── nexl-infer (M2)
├── nexl-types (M2)
├── nexl-ir (M8)
├── nexl-wasm (M8)
├── nexl-vm (M8)
└── nexl-runtime (M1)

nexl-infer (M2)
├── nexl-ast
├── nexl-types
└── nexl-errors

nexl-effects (M6)
├── nexl-types
├── nexl-infer
└── nexl-errors
```

New crates are added as their milestone begins. This file is updated accordingly.
