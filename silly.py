import random
import sys
print("ready")
sys.stdout.flush()
while True:
    line = input()
    if line == "game":
        guess = random.randint(0, 5)
        print(guess)
        sys.stdout.flush()
        _all = map(int, input().split())
    elif line == "end":
        break