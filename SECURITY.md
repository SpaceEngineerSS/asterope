# Security

Asterope reads files and frames that may come from elsewhere: catalogs,
indexes, FITS files, images. A way to make any of those readers panic, read
out of bounds, loop without end or take unbounded memory is a security
problem here, and so is anything in the C interface that breaks memory
safety when it is used as documented.

Report it privately through "Report a vulnerability" on the repository's
Security tab. Say which version or commit, what input, and what happened.
A report gets an answer within a week.

Only the latest release is fixed.
