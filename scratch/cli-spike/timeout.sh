#!/bin/sh
# Substituto de coreutils `timeout` (ausente no macOS base).
# Uso: timeout.sh <segundos> <cmd> [args...]   (stdin e' repassado ao cmd)
exec perl -e 'my $t=shift; alarm $t; exec @ARGV or die "exec: $!"' "$@"
