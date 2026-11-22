// Char-level diff (LCS over code points) between a misspelled word and a candidate.
// Returns two render streams: `left` for the original (match | del) and `right` for the
// candidate (match | ins). Words are short, so O(m·n) is negligible.

export type DiffChar = { ch: string; type: "match" | "del" | "ins" };

export function charDiff(a: string, b: string): { left: DiffChar[]; right: DiffChar[] } {
  const A = [...a];
  const B = [...b];
  const m = A.length;
  const n = B.length;
  // dp[i][j] = length of the longest common subsequence of A[i..] and B[j..]
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      dp[i][j] = A[i] === B[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const left: DiffChar[] = [];
  const right: DiffChar[] = [];
  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (A[i] === B[j]) {
      left.push({ ch: A[i], type: "match" });
      right.push({ ch: B[j], type: "match" });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      left.push({ ch: A[i], type: "del" });
      i++;
    } else {
      right.push({ ch: B[j], type: "ins" });
      j++;
    }
  }
  while (i < m) left.push({ ch: A[i++], type: "del" });
  while (j < n) right.push({ ch: B[j++], type: "ins" });
  return { left, right };
}
