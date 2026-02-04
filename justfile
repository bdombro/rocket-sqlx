set dotenv-load

# Default command: runs `just -h`
_: 
  @just -l

benchmark-init:
  brew install hey

# performance benchmark the app with a simple cookie-authenticated GET request.
# 70-85k req/s in production mode on M4 Pro macbook
benchmark-cookie:
  hey -n 10000 -c 50 -m GET \
    -H "Cookie: user_id=$USER_ID_COOKIE" \
    http://127.0.0.1:8000/api/session

# performance benchmark the app with a single DB read
# 48-57k req/s in production mode on M4 Pro macbook
benchmark-r:
  hey -n 100000 -c 50 -m GET \
    -H "Cookie: user_id=$USER_ID_COOKIE" \
    http://127.0.0.1:8000/api/posts?limit=1

# performance benchmark the app with a single DB write
# 9-11.5k req/s in production mode on M4 Pro macbook
benchmark-w:
  hey -n 50000 -c 50 -m POST \
    -H "Cookie: user_id=$USER_ID_COOKIE" \
    -H "Content-Type: application/json" \
    -d '{"content":"Benchmarking POST","variant":"note"}' \
    http://127.0.0.1:8000/api/posts

# performance benchmark the app with 1/10 writes per read
# ~3.16s, 31600r/s total, Write 3200r/s, Read 29200r/s in 
# production mode on M4 Pro macbook
benchmark-rw:
  #!/bin/bash
  # Save the start time
  start_time=$(date +%s.%N)

  # Run GET and POST benchmarks simultaneously
  hey -n 90000 -c 50 -m GET \
    -H "Cookie: user_id=$USER_ID_COOKIE" \
    http://127.0.0.1:8000/api/posts?limit=1 &
  hey -n 10000 -c 50 -m POST \
    -H "Cookie: user_id=$USER_ID_COOKIE" \
    -H "Content-Type: application/json" \
    -d '{"content":"Benchmarking POST","variant":"note"}' \
    http://127.0.0.1:8000/api/posts &
  wait

  # Calculate and print the elapsed time
  end_time=$(date +%s.%N)
  elapsed_time=$(printf "%.2f" "$(bc -l <<< "${end_time} - ${start_time}")")
  req_per_sec=$(printf "%.2f" "$(bc -l <<< "100000 / ${elapsed_time}")")
  echo "Benchmark completed in ${elapsed_time} seconds"
  echo "req/s = ${req_per_sec}"

# Build the project in debug mode
build:
  cargo build --release

# Check the code for errors and ensure formatting is correct
check:
  cargo check
  cargo fmt -- --config max_width=120 --check

alias prepare := codegen
# Gen DB query typings
codegen:
  cargo sqlx prepare

# Run the application in debug mode with verbose logging
debug:
  ROCKET_LOG_LEVEL=debug cargo run

alias fmt := format
# Format the codebase
format:
  cargo fmt -- --config max_width=120

# Will set everything up for a clean repo
init:
  echo just precommit > .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
  cp .env.example .env
  just keygen-dkim
  just keygen-cookie-secret
  cargo install sqlx-cli --no-default-features --features sqlite
  just reset
  just prepare

# Generate DKIM keys and store them in the .env file
keygen-dkim:
  #!/bin/bash
  openssl genrsa -out priv.tmp 2048
  openssl rsa -in priv.tmp -pubout -out pub.tmp
  openssl pkcs8 -topk8 -inform PEM -outform PEM -nocrypt -in priv.tmp -out pkcs8.tmp
  PRI_STR="DKIM_KEY_PRIVATE=\"$(awk '{printf "%s\\n", $0}' pkcs8.tmp)\""
  PUB_STR="DKIM_KEY_PUBLIC=\"$(awk '{printf "%s\\n", $0}' pub.tmp)\""
  PRI_SED=$(echo "$PRI_STR" | sed 's/\\/\\\\/g')
  PUB_SED=$(echo "$PUB_STR" | sed 's/\\/\\\\/g')
  grep -q "^DKIM_KEY_PRIVATE=" .env && sed -i '' "s|^DKIM_KEY_PRIVATE=.*|$PRI_SED|" .env || echo "$PRI_STR" >> .env
  grep -q "^DKIM_KEY_PUBLIC=" .env && sed -i '' "s|^DKIM_KEY_PUBLIC=.*|$PUB_SED|" .env || echo "$PUB_STR" >> .env
  rm priv.tmp pub.tmp pkcs8.tmp
  echo "Keys rotated in .env successfully."

# Generate a random cookie secret key and update the .env file
keygen-cookie-secret:
  #!/bin/bash
  KEY="$(openssl rand -base64 32)"
  grep -q "^ROCKET_SECRET_KEY=" .env && sed -i '' "s|^ROCKET_SECRET_KEY=.*|ROCKET_SECRET_KEY=$KEY|" .env || echo "ROCKET_SECRET_KEY=$KEY" >> .env

# Run database migrations
migrate:
  cargo sqlx migrate run

# Generate a new database migration with the given name
migrate-generate name:
  cargo sqlx migrate add {{name}}

# Run checks and tests before committing code
precommit:
  just check
  just test

# Reset the database by delete then create+migrate
reset:
  rm -f db.sqlite*
  cargo sqlx database reset -y
  just migrate

update:
  cargo update

alias up := start
alias run := start
# Start the application in debug mode
start:
  cargo run

alias prod := start-prod
alias run-prod := start-prod
# Start the application in release mode
start-prod:
  ./target/release/rocket-sqlx

# Run all tests
test:
  cargo test
