# Heap1

The simplest possible heap. It's similar to [heap1 in FreeRTOS](https://github.com/FreeRTOS/FreeRTOS-Kernel/blob/main/portable/MemMang/heap_1.c).

Because it's the simplest implementation, it does **NOT** free memory.
Any memory you drop cannot be reused (it's leaked), so avoid dropping anything whenever possible. It is recommended that you use [embedded-alloc](https://crates.io/crates/embedded-alloc)

## Usage
```sh
cargo add heap1
```

### Global Allocator

Using static global allocator:

```rust
use heap1::{Heap, Inline};

#[global_allocator]
static HEAP: Heap<Inline<100>> = Heap::new();
```

You can also initialize the global allocator in two steps to meet specific requirements:

```rust
use core::mem::MaybeUninit;
use heap1::{Heap, Pointer};

#[global_allocator]
static HEAP: Heap<Pointer> = Heap::empty();

fn main() {
    // Initialize the allocator BEFORE you use it
    const HEAP_SIZE: usize = 100;
    static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
    unsafe { HEAP.init_with_ptr(&raw mut HEAP_MEM as usize, HEAP_SIZE) }
}
```

### Local Allocator

Create a local allocator on stack.

```rust
use heap1::{Heap, Inline};

fn foo() {
    let heap = Heap::<Inline::<100>>::new();
}
```

Create a local allocator from global heap.

```rust
use heap1::Heap;

fn foo() {
    let heap = Heap::new_boxed(64);
}
```

## Cargo Features
- `std` for unit test only
- `allocator-api` for unstable allocator-api
