"""Generate reproducible BZip2 benchmark inputs outside the repository."""
import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--mib", type=int, default=32)
    args = parser.parse_args()
    if args.mib < 1:
        parser.error("size must be positive")
    if args.destination.resolve().is_relative_to(Path(__file__).resolve().parents[3]):
        parser.error("generated corpora must remain outside the repository")
    args.destination.mkdir(parents=True, exist_ok=True)
    block_size = 1024 * 1024
    text = b"A reproducible compression corpus: records, words, numbers 0123456789.\n"
    state = 0x12345678
    random = bytearray()
    for _ in range(block_size):
        state ^= (state << 13) & 0xffffffff
        state ^= state >> 17
        state ^= (state << 5) & 0xffffffff
        random.append(state & 255)
    # The random block repeats at 1 MiB, beyond a BZip2 block's history.
    blocks = {
        "text": (text * (block_size // len(text) + 1))[:block_size],
        "random": random,
        "runs": ((bytes([42]) * 64 + bytes([17])) * (block_size // 65 + 1))[:block_size],
        "mixed": bytes(random[:block_size // 2]) + (bytes(range(256)) * 2048),
    }
    for name, block in blocks.items():
        with (args.destination / (name + ".bin")).open("wb") as output:
            for _ in range(args.mib):
                output.write(block)


if __name__ == "__main__":
    main()
