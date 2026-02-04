# rocket-sqlx

A rust REST API featuring rocket, email auth, sqlite, CRUD.

For rest enpoints, see the `src/handlers` directory. For utility functions, see `src/util.rs`.

# Getting Started

There are many useful commands in the [justfile](https://just.systems/). To get started, install just and you can run `just init` to set up the .env file, generate DKIM keys, and prepare the database. Then you can run `just start` to start the app.

## Performance

The results of the benchmarks in the justfile were observed on a MacBook M4 Pro in production mode:

- **`benchmark-cookie`**: Authenticated GET request to `/api/session` achieves **70-85k req/s**.
- **`benchmark-r`**: Authenticated single database read (GET `/api/posts?limit=1`) achieves **48-57k req/s**.
- **`benchmark-w`**: Authenticated single database write (POST `/api/posts`) achieves **9-11.5k req/s**.
- **`benchmark-rw`**: Mixed workload (90% reads, 10% writes) achieves **31.6k req/s total**:
  - Reads: **29.2k req/s**
  - Writes: **3.2k req/s**

Comparing the raw req/s (as in no db read/write) speed between languages:

Rust - 85k - 120k
Go - 45k - 70k
Java - 40k - 80k
Node.js - 15k - 30k
Python - 1k - 4k
