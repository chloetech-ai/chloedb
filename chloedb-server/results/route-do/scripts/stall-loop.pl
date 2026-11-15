# perl stall-loop.pl PID STOP_MS PERIOD_MS: the process frozen STOP_MS of each PERIOD_MS.
# Sleeps with select() (core perl): the pods' perl-base has no Time::HiRes.
my ($pid, $stop, $period) = @ARGV;
$0 = "stall-loop";
$SIG{TERM} = sub { kill 'CONT', $pid; exit 0 };
sub nap { select(undef, undef, undef, $_[0] / 1000) }
while (1) { nap($period - $stop); kill('STOP', $pid) or exit 1; nap($stop); kill 'CONT', $pid; }
