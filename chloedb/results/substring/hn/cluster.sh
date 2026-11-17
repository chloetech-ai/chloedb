#!/bin/bash
# cluster.sh BIN DIR start|stop: 4 shards and a coordinator on this machine, as hn4 runs them.
BIN=$1; DIR=$2; CMD=$3
mkdir -p $DIR
if [ "$CMD" = stop ]; then
  for f in $DIR/*.pid; do [ -f $f ] && kill $(cat $f) 2>/dev/null; done; sleep 2; rm -f $DIR/*.pid; exit 0
fi
for i in 0 1 2 3; do
  nohup $BIN --db $DIR/s$i.chloedb --seq-no --listen 127.0.0.1:78${i}0 --grpc 127.0.0.1:77${i}1 --answer-cache-mb 0 > $DIR/s$i.log 2>&1 &
  echo $! > $DIR/s$i.pid
done
sleep 3
nohup $BIN --shards http://127.0.0.1:7701,http://127.0.0.1:7711,http://127.0.0.1:7721,http://127.0.0.1:7731 --listen 127.0.0.1:7700 --grpc off --answer-cache-mb 0 > $DIR/coord.log 2>&1 &
echo $! > $DIR/coord.pid
for t in $(seq 1 60); do curl -sf 127.0.0.1:7700/v1/health >/dev/null && break; sleep 1; done
curl -s 127.0.0.1:7700/v1/health | head -c 300; echo
