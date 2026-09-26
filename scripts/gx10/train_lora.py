#!/usr/bin/env python3
"""
LoRA supervised fine-tune of a vision-language model on statement-page -> rows JSON.

Data: JSONL from scripts/gx10/synth_dataset.py (image, prompt, answer).
Model: any HF image-text-to-text checkpoint with a chat template (e.g. a Qwen-VL
family model) passed via --model. No default: pick one whose licence and size you
have verified for the GX10's 128 GB.

STATUS: written for the GX10 (aarch64 + CUDA, transformers + peft). It has NOT
been run - this dev box has no GPU and no peft. Smoke-test with --max-steps 5
before a real run, and read the loss masking note below.

Loss is computed on the assistant answer only: the prompt+image tokens are masked
by tokenising the prompt with add_generation_prompt=True and masking that prefix.
That relies on the chat template producing an identical prefix for both calls;
the script asserts it.

  pip install "transformers>=4.57" peft accelerate pillow
  python scripts/gx10/train_lora.py --model <hf-id-or-path> --data data/synth --out runs/lora1
  python scripts/gx10/train_lora.py ... --merge     # also write a merged model for vLLM
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path


def load_jsonl(path: Path) -> list:
    with path.open(encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def build_example(processor, root: Path, rec: dict, max_len: int):
    from PIL import Image

    image = Image.open(root / rec["image"]).convert("RGB")
    user = {"role": "user", "content": [{"type": "image"}, {"type": "text", "text": rec["prompt"]}]}
    asst = {"role": "assistant", "content": [{"type": "text", "text": rec["answer"]}]}

    def enc(messages, gen_prompt):
        text = processor.apply_chat_template(messages, tokenize=False, add_generation_prompt=gen_prompt)
        return processor(text=[text], images=[image], return_tensors="pt")

    prompt_enc = enc([user], True)
    full = enc([user, asst], False)
    n_prompt = prompt_enc["input_ids"].shape[1]
    if not (full["input_ids"][0, :n_prompt] == prompt_enc["input_ids"][0]).all():
        raise RuntimeError("chat template prefix mismatch: cannot mask the prompt reliably")
    if full["input_ids"].shape[1] > max_len:
        return None  # too long for the configured budget; caller counts and skips
    labels = full["input_ids"].clone()
    labels[:, :n_prompt] = -100
    full["labels"] = labels
    return full


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model", required=True)
    ap.add_argument("--data", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--epochs", type=float, default=2.0)
    ap.add_argument("--lr", type=float, default=1e-4)
    ap.add_argument("--rank", type=int, default=16)
    ap.add_argument("--grad-accum", type=int, default=8)
    ap.add_argument("--max-len", type=int, default=6144)
    ap.add_argument("--max-steps", type=int, default=0, help="stop after N optimizer steps (smoke test)")
    ap.add_argument("--seed", type=int, default=1234)
    ap.add_argument("--merge", action="store_true")
    args = ap.parse_args()

    try:
        import torch
        from peft import LoraConfig, get_peft_model
        from transformers import AutoModelForImageTextToText, AutoProcessor
    except ImportError as exc:
        print(f"missing dependency: {exc}", file=sys.stderr)
        return 2
    if not torch.cuda.is_available():
        print("CUDA not available; refusing to train on CPU", file=sys.stderr)
        return 2

    random.seed(args.seed)
    torch.manual_seed(args.seed)
    train = load_jsonl(args.data / "train.jsonl")
    if not train:
        print("empty train.jsonl", file=sys.stderr)
        return 2

    processor = AutoProcessor.from_pretrained(args.model)
    model = AutoModelForImageTextToText.from_pretrained(
        args.model, dtype=torch.bfloat16, device_map="cuda"
    )
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    model = get_peft_model(
        model,
        LoraConfig(r=args.rank, lora_alpha=2 * args.rank, lora_dropout=0.05,
                   target_modules="all-linear", task_type="CAUSAL_LM"),
    )
    model.print_trainable_parameters()
    params = [p for p in model.parameters() if p.requires_grad]
    opt = torch.optim.AdamW(params, lr=args.lr, weight_decay=0.0)

    total_micro = int(len(train) * args.epochs)
    total_steps = max(1, total_micro // args.grad_accum)
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda s: min(1.0, (s + 1) / 10) * max(0.05, 1 - s / total_steps))

    model.train()
    skipped = step = micro = 0
    running = 0.0
    order: list = []
    while micro < total_micro:
        if not order:
            order = random.sample(range(len(train)), len(train))
        batch = build_example(processor, args.data, train[order.pop()], args.max_len)
        if batch is None:
            skipped += 1
            micro += 1
            continue
        batch = {k: v.to("cuda") for k, v in batch.items()}
        loss = model(**batch).loss / args.grad_accum
        loss.backward()
        running += loss.item()
        micro += 1
        if micro % args.grad_accum == 0:
            torch.nn.utils.clip_grad_norm_(params, 1.0)
            opt.step()
            sched.step()
            opt.zero_grad(set_to_none=True)
            step += 1
            print(f"step {step}/{total_steps} loss {running:.4f} skipped_long={skipped}", flush=True)
            running = 0.0
            if args.max_steps and step >= args.max_steps:
                break

    args.out.mkdir(parents=True, exist_ok=True)
    model.save_pretrained(args.out / "adapter")
    processor.save_pretrained(args.out / "adapter")
    if args.merge:
        merged = model.merge_and_unload()
        merged.save_pretrained(args.out / "merged")
        processor.save_pretrained(args.out / "merged")
    print(f"saved to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
