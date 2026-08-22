#!/usr/bin/env bash
# Ladder driver.
#  * single instance, enforced by a lock file
#  * checkpoint granularity = one class, appended the moment it is known
#  * solve.exe reports EVERY class in the dependency closure it solves, so we
#    record all of them: the recomputation becomes recorded progress instead of
#    wasted work, and later classes get skipped.
R="C:/Woodchop/Code/Microchess"
LADDER="$R/run/ladder.txt"; RES="$R/run/results.tsv"; LOG="$R/run/run.log"
LOCK="$R/run/driver.lock"; SOLVE="$R/solver/target/release/solve.exe"
CAP=${CAP:-400000000}; TMO=${TMO:-1800}
if ! ( set -o noclobber; echo "$$" > "$LOCK" ) 2>/dev/null; then
  echo "driver already running (lock $LOCK held by $(cat "$LOCK" 2>/dev/null))"; exit 1
fi
trap 'rm -f "$LOCK"' EXIT INT TERM
echo "=== driver started $(date -Iseconds) pid=$$ cap=$CAP tmo=${TMO}s ===" >> "$LOG"
declare -A SEEN
while IFS=$'\t' read -r n rest; do [ -n "$n" ] && SEEN[$n]=1; done < <(tail -n +2 "$RES" | cut -f1)
while IFS=$'\t' read -r name pieces slots; do
  [ -z "$name" ] && continue
  [ -n "${SEEN[$name]}" ] && continue
  if [ "$slots" -gt "$CAP" ]; then echo "$(date +%T) SKIP $name ($slots slots)" >> "$LOG"; continue; fi
  t0=$(date +%s)
  raw=$(timeout "$TMO" "$SOLVE" "$name" 2>&1)
  t1=$(date +%s)
  got=0
  while read -r rline; do
    [ -z "$rline" ] && continue
    cls=$(printf '%s' "$rline" | sed -E 's/^\[retro\] ([A-Za-z]+):.*/\1/')
    [ -n "${SEEN[$cls]}" ] && continue
    vals=$(printf '%s\n' "$rline" | awk '{for(i=1;i<=NF;i++) v[$i]=$(i+1);
      printf "%s\t%s\t%s\t%s\t%s\t%s", v["positions"],v["win"],v["loss"],v["draw"],v["illegal"],v["iters"]}')
    pc=$(( $(printf '%s' "$cls" | tr -cd 'A-Z' | wc -c) ))
    printf '%s\t%s\t%s\t%s\t%s\n' "$cls" "$pc" "?" "$vals" "$((t1-t0))" >> "$RES"
    SEEN[$cls]=1; got=$((got+1))
  done < <(printf '%s\n' "$raw" | grep -E '^\[retro\] [A-Za-z]+: positions')
  if [ "$got" -gt 0 ]; then
    echo "$(date +%T) OK   $name ${pieces}p ${got} classes recorded in $((t1-t0))s" >> "$LOG"
  else
    echo "$(date +%T) FAIL $name ${pieces}p after $((t1-t0))s :: $(printf '%s' "$raw" | tail -1 | cut -c1-90)" >> "$LOG"
    SEEN[$name]=1
    printf '%s\t%s\t%s\tFAILED\t\t\t\t\t\t%s\n' "$name" "$pieces" "$slots" "$((t1-t0))" >> "$RES"
  fi
done < "$LADDER"
echo "=== driver finished $(date -Iseconds) ===" >> "$LOG"
