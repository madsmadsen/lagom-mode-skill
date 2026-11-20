# rust-todo

A small command-line to-do list. It is the sample project of the skills PRD, section 9. The sample
features run on this project later.

It has no dependencies. It is its own Cargo project, not a member of a workspace.

## Use

```bash
cargo run -- add buy milk
cargo run -- list
```

`add <title>` stores a task and prints its id. `list` prints every task, one per line, then a
summary. Ids count from 1 and never change.

## The store

The store is a plain text file. One task per line, with the id and the title separated by a tab.
`TODO_FILE` names the file. Without that variable the file is `todo.txt` in the current directory.

```bash
TODO_FILE=/tmp/my-list.txt cargo run -- add walk dog
```

## The code

| File | Content |
| --- | --- |
| `src/main.rs` | Reads the arguments and the store path, calls the library, sets the exit code. |
| `src/lib.rs` | The `add` and `list` commands. |
| `src/task.rs` | One task, and how a line of the store is read and written. |
| `src/store.rs` | Load, save, and the next id. |
| `src/summary.rs` | The sentence after the list. |

`src/summary.rs` holds one complex function on purpose, as AC-14 of the skills PRD requires. The
sample features must not touch that function.
