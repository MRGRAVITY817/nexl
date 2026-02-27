# M22 — Collections & Algorithms

## Deliverables

- [x] 1. `sort` / `sort-by`
  - `(sort [3 1 2])` → `[1 2 3]`
  - `(sort-by f coll)` — sorted by key function
  - Stable sort

- [x] 2. `reverse`
  - `(reverse [1 2 3])` → `[3 2 1]`

- [x] 3. Bitwise operations
  - `bit-and`, `bit-or`, `bit-xor`, `bit-not`, `bit-shift-left`, `bit-shift-right`

- [x] 4. `range`
  - `(range 5)` → `[0 1 2 3 4]`
  - `(range 1 10)` → `[1 2 ... 9]`
  - `(range 0 10 2)` → `[0 2 4 6 8]`

- [x] 5. `flat-map` / `mapcat`
  - `(flat-map f coll)` — map then flatten one level

- [x] 6. `group-by`
  - `(group-by f coll)` → Map from key to Vec of matching elements

- [x] 7. `zip`
  - `(zip [1 2 3] ["a" "b" "c"])` → `[[1 "a"] [2 "b"] [3 "c"]]`

- [x] 8. `take` / `drop` / `take-while` / `drop-while`

- [x] 9. `str/format`
  - `(str/format "Hello, {}!" name)` — positional placeholder formatting

- [ ] 10. HashMap-backed Map (stretch goal)
  - Replace O(n) Vec-backed Map with O(1) HashMap for large maps
