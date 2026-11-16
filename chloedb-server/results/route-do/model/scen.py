"""The scenarios as model inputs, built a priori from what each injects
(no parameter fitted on the scenario judged)."""
import json, os, random
import sim
FIT = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "fit.json")))
# A copy beside k CPU-bound threads (stress-ng --cpu k) on its 2-vCPU node:
# CFS shares the 2 vCPUs among the server's runnable threads and the k
# others (sim.Copy), and each server thread runs slower while the other
# hyperthread is busy. That slowdown is the one number taken from a stress
# scenario: copy 0's light-load service time under 1 thread (cpuhalf1,
# 1 client: 0.95 ms against 0.69 ms on copies alike). The same factor
# serves cpubusy (2 threads), whose own runs stay out of every fit.
HT = 0.69 / 0.95
STRESS = {"cpuhalf": 1, "cpubusy": 2}
CONFIGS = {"round-robin": ("round-robin", False), "round-robin adaptive": ("round-robin", True),
           "least-cost": ("least-cost", False), "least-cost adaptive": ("least-cost", True),
           "least-cost no-load": ("least-cost", False)}

def params(scenario, clients, config, seed=0, fit=FIT, shards=None, copies=3, seconds=None, **over):
    base = scenario.rstrip("1")
    S = shards or (1 if scenario.endswith("1") else 3)
    route, adaptive = CONFIGS[config]
    C = fit["cores"]
    cores = [[C] * copies for _ in range(S)]
    speed = [[1.0] * copies for _ in range(S)]
    speed[0][1] = fit["h1"]          # node c01, faster in every measured run
    slots = [[64] * copies for _ in range(S)]
    pause, extra, stress = {}, {}, {}
    rng = random.Random(seed * 7919 + 13)
    if base == "busy":
        slots[0][0] = 1
    elif base in STRESS:
        speed[0][0] = HT
        stress[(0, 0)] = STRESS[base]
    elif base == "sigstop":
        pause[(0, 0)] = (1000.0, 60.0, rng.uniform(0, 1000))
    elif base == "stall":
        extra[(0, 0)] = sim.stall_extra(rng.uniform(0, 1000))
    elif base == "slow":
        extra[(0, 0)] = lambda now: 30.0
    p = dict(shards=S, copies=copies, cores=cores, speed=speed, slots=slots, pause=pause, extra=extra, stress=stress,
             n_half=fit["n_half"],
             route=route, adaptive=adaptive, clients=clients, warmup=1500.0,
             seconds=seconds or (10000.0 if clients == 1 else 4000.0),
             net_med=fit["net_med"], net_sigma=fit["net_sigma"], d_med=fit["d_med"], d_sigma=fit["d_sigma"])
    p.update(over)
    return p
