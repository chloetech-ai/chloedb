// bbsweep-go: runs Nodwyn's L1 detector (l1/l1.go, an unmodified copy of
// internal/detect/l1/l1.go at commit cdc3006, Apache-2.0) and the gitleaks rules (their
// regexes on Go's regexp, the engine gitleaks runs them on) over JSON Lines of text, and
// prints the spans each would remove.
//
//	bbsweep-go <provider-formats.jsonl> < docs.jsonl > spans.jsonl
//	bbsweep-go <provider-formats.jsonl> --self     # each gitleaks example and what its rule captures
//
// A gitleaks rule is applied as gitleaks applies it: only to text that holds one of its
// keywords (compared in lower case), its secret is the first capture group that took part
// (else the match), and a match whose secret has less Shannon entropy than the rule asks is
// dropped. gitleaks's allowlists and stopwords are not applied.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"regexp"
	"runtime"
	"strings"
	"sync"

	"bbsweep/l1"
)

type ruleIn struct {
	Source   string   `json:"source"`
	ID       string   `json:"id"`
	Regex    string   `json:"regex"`
	Keywords []string `json:"keywords"`
	Entropy  *float64 `json:"entropy"`
	Examples []string `json:"examples"`
}

type rule struct {
	ruleIn
	re *regexp.Regexp
}

type doc struct {
	ID   string `json:"id"`
	Text string `json:"text"`
}

type span [3]any

func entropy(s string) float64 {
	if s == "" {
		return 0
	}
	var freq [256]int
	for i := 0; i < len(s); i++ {
		freq[s[i]]++
	}
	e, n := 0.0, float64(len(s))
	for _, c := range freq {
		if c > 0 {
			p := float64(c) / n
			e -= p * math.Log2(p)
		}
	}
	return e
}

// secretOf is the span of a match's secret: the first capture group that took part.
func secretOf(m []int) (int, int) {
	for g := 2; g+1 < len(m); g += 2 {
		if m[g] >= 0 && m[g+1] > m[g] {
			return m[g], m[g+1]
		}
	}
	return m[0], m[1]
}

func (r *rule) find(text, lower string) [][2]int {
	if len(r.Keywords) > 0 {
		hit := false
		for _, k := range r.Keywords {
			if strings.Contains(lower, strings.ToLower(k)) {
				hit = true
				break
			}
		}
		if !hit {
			return nil
		}
	}
	var out [][2]int
	for _, m := range r.re.FindAllStringSubmatchIndex(text, -1) {
		s, e := secretOf(m)
		if r.Entropy != nil && entropy(text[s:e]) < *r.Entropy {
			continue
		}
		out = append(out, [2]int{s, e})
	}
	return out
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: bbsweep-go <provider-formats.jsonl> [--self]")
		os.Exit(2)
	}
	self := len(os.Args) > 2 && os.Args[2] == "--self"
	f, err := os.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	var rules []*rule
	var refused []map[string]string
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 64<<20)
	for sc.Scan() {
		var in ruleIn
		if json.Unmarshal(sc.Bytes(), &in) != nil || in.Source != "gitleaks" {
			continue
		}
		if in.Regex == "" {
			continue // a path rule (pkcs12-file): it reads file names, not text
		}
		re, err := regexp.Compile(in.Regex)
		if err != nil {
			refused = append(refused, map[string]string{"id": in.ID, "error": err.Error()})
			continue
		}
		rules = append(rules, &rule{in, re})
	}
	out := bufio.NewWriterSize(os.Stdout, 1<<20)
	defer out.Flush()
	enc := json.NewEncoder(out)
	enc.SetEscapeHTML(false)
	if self {
		for _, r := range rules {
			for n, ex := range r.Examples {
				var secret any
				if m := r.find(ex, strings.ToLower(ex)); len(m) > 0 {
					secret = ex[m[0][0]:m[0][1]]
				}
				enc.Encode(map[string]any{"rule": r.ID, "n": n, "example": ex, "secret": secret})
			}
		}
		enc.Encode(map[string]any{"compiled": len(rules), "refused": refused})
		return
	}
	// Documents are scanned on every core, and answered in the order they came.
	type job struct {
		n    int
		line []byte
	}
	type res struct {
		n   int
		out map[string]any
	}
	jobs := make(chan job, 256)
	results := make(chan res, 256)
	var wg sync.WaitGroup
	for w := 0; w < runtime.NumCPU(); w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			scanner := l1.New()
			for j := range jobs {
				var d doc
				if json.Unmarshal(j.line, &d) != nil {
					results <- res{j.n, nil}
					continue
				}
				nod := []span{}
				for _, fd := range scanner.Scan(d.Text) {
					// The unknown-entropy residual is found but not redacted (internal/redact/redact.go).
					nod = append(nod, span{fd.Start, fd.End, fd.Class})
				}
				gl := []span{}
				lower := strings.ToLower(d.Text)
				for _, r := range rules {
					for _, m := range r.find(d.Text, lower) {
						gl = append(gl, span{m[0], m[1], r.ID})
					}
				}
				results <- res{j.n, map[string]any{"id": d.ID, "nodwyn": nod, "gitleaks": gl}}
			}
		}()
	}
	go func() {
		in := bufio.NewScanner(os.Stdin)
		in.Buffer(make([]byte, 1<<20), 256<<20)
		n := 0
		for in.Scan() {
			line := append([]byte(nil), in.Bytes()...)
			if len(strings.TrimSpace(string(line))) == 0 {
				continue
			}
			jobs <- job{n, line}
			n++
		}
		close(jobs)
		wg.Wait()
		close(results)
	}()
	pending := map[int]map[string]any{}
	next := 0
	for r := range results {
		pending[r.n] = r.out
		for {
			o, ok := pending[next]
			if !ok {
				break
			}
			delete(pending, next)
			next++
			if o != nil {
				enc.Encode(o)
			}
		}
	}
}
