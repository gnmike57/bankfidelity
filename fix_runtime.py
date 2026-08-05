import re

with open('src/app/runtime.rs', 'r') as f:
    text = f.read()

# Fix parse_matrix
text = text.replace(
    'tasks.push(tokio::spawn(async move {\n                                        ("DocAI", crate::engine::pro_edit::perform_pro_edit("DocumentAI", async { doc_ai.parse_entire_statement(&p, None).await.map_err(|e| anyhow::anyhow!(e)) }, wdog.clone()).await.ok())\n                                    }));',
    'let wdog_docai = wdog.clone();\n                                    tasks.push(tokio::spawn(async move {\n                                        ("DocAI", crate::engine::pro_edit::perform_pro_edit("DocumentAI", async { doc_ai.parse_entire_statement(&p, None).await.map_err(|e| anyhow::anyhow!(e)) }, wdog_docai).await.ok())\n                                    }));'
)

text = text.replace(
    'tasks.push(tokio::spawn(async move {\n                                        ("LlamaParse", crate::engine::pro_edit::perform_pro_edit("LlamaParse", async { llama.parse_statement(&p).await.map_err(|e| anyhow::anyhow!(e)) }, wdog.clone()).await.ok())\n                                    }));',
    'let wdog_llama = wdog.clone();\n                                    tasks.push(tokio::spawn(async move {\n                                        ("LlamaParse", crate::engine::pro_edit::perform_pro_edit("LlamaParse", async { llama.parse_statement(&p).await.map_err(|e| anyhow::anyhow!(e)) }, wdog_llama).await.ok())\n                                    }));'
)

# Fix RunTransferTests loop
# Wait, RunTransferTests is an async block inside a tokio::spawn.
# I will use a regex to find all occurrences of `perform_pro_edit("DocumentAI"` and `perform_pro_edit("LlamaParse"` and `perform_pro_edit("Gemini"` that capture `wdog.clone()` inside `tokio::spawn(async move {`
# This might be tricky. Let's just fix the specific occurrences.

# In ExtractTransactions
text = text.replace(
    'crate::engine::pro_edit::perform_pro_edit("LlamaParse", async { client.parse_statement(&path).await.map_err(|e| anyhow::anyhow!(e)) }, wdog.clone()).await',
    'crate::engine::pro_edit::perform_pro_edit("LlamaParse", async { client.parse_statement(&path).await.map_err(|e| anyhow::anyhow!(e)) }, wdog.clone()).await'
) # This is fine if wdog is not moved. But if `wdog` is moved into `async move`, then `.clone()` is called on it after it was moved?
# Ah, `wdog.clone()` inside `async move` evaluates when the future is constructed? No, `wdog` is moved into the closure `async move`, and THEN inside the closure we do `wdog.clone()`. This means `wdog` is moved.
# So ANY `tokio::spawn(async move { ... wdog.clone() ... })` moves `wdog`.
# Since `wdog` is created once in the `while` loop via `let wdog = watchdog_clone.clone();`, ANY `tokio::spawn` inside the `match job {` that captures `wdog` will MOVE it. Since it's a loop, moving `wdog` is fine, EXCEPT when a single `job` matches and does MULTIPLE `tokio::spawn` or a `while/for` loop that captures `wdog`.

# 1. Job::ExtractTransactions does not spawn multiple tokio tasks. It spawns ONE `tokio::spawn` and inside it, we use `wdog`. But wait! `wdog` is moved into `tokio::spawn`. Inside `tokio::spawn`, we have multiple `if let Ok(client) = ...` where we do `perform_pro_edit(..., wdog.clone())`. So `wdog` is captured by `tokio::spawn` and moved there. That's fine. Wait, does it capture `wdog` by move and then reuse it? Yes, `wdog.clone()` inside the block creates a clone. So why does `parse_matrix` fail?
# `parse_matrix` fails because `parse_matrix` has multiple `tokio::spawn(async move { ... })`.

# 2. Line 4770 failure: inside `Job::RunTransferTests`. There is a loop:
# `for entry in std::fs::read_dir(&data_dir).unwrap() {`
# Inside this loop, it spawns tasks? Or it iterates and uses `wdog`?
# Ah! Inside the `for` loop, it might be spawning `async move`, or just doing `.await`. Wait, it's `async move`! It says `value moved here, in previous iteration of loop`.
# Let's check `RunTransferTests`.

with open('src/app/runtime.rs', 'w') as f:
    f.write(text)
