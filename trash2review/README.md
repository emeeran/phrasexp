# trash2review

Holding pen for files flagged as redundant or dead by the cleanup pipeline.
**Nothing here is deleted** — files are `git mv`'d here so a human can review and
decide. To restore a file to its original location:

```
git mv trash2review/<original-relative-path> <original-relative-path>
```

The mirrored directory structure preserves each file's original path.
If this folder is empty (or absent), nothing is pending review.
