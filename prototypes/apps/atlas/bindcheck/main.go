// Command atlasbind checks the MCP Atlas's generated bindings.toml rows with
// Nodwyn's own parser (binding.Parse, copied unmodified into ./binding).
//
// Input, one JSON object per line on stdin: the server, the TOML the Atlas
// shows for it, and the rows it meant to write.
//
//	{"server":"io.github.x/y","toml":"[[bind]]\n...","want":[{"class":..,"dest":..,"positions":[..],"methods":[..]}]}
//
// Output, one line per input: whether Parse accepted the table, its error,
// how many rows it decoded, and for each wanted row whether Parse returned
// exactly that row (same class, dest, positions, methods) in that order.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"slices"

	"atlasbind/binding"
)

type row struct {
	Class     string   `json:"class"`
	Dest      string   `json:"dest"`
	Positions []string `json:"positions"`
	Methods   []string `json:"methods"`
}

type in struct {
	Server string `json:"server"`
	TOML   string `json:"toml"`
	Want   []row  `json:"want"`
}

type out struct {
	Server  string `json:"server"`
	OK      bool   `json:"ok"`
	Err     string `json:"err,omitempty"`
	Decoded int    `json:"decoded"`
	RowOK   []bool `json:"row_ok"`
}

func main() {
	sc := bufio.NewScanner(os.Stdin)
	sc.Buffer(make([]byte, 1<<20), 16<<20)
	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	enc := json.NewEncoder(w)
	for sc.Scan() {
		var q in
		if err := json.Unmarshal(sc.Bytes(), &q); err != nil {
			fmt.Fprintln(os.Stderr, "atlasbind: bad input line:", err)
			os.Exit(2)
		}
		r := out{Server: q.Server, RowOK: make([]bool, len(q.Want))}
		t, err := binding.Parse([]byte(q.TOML))
		if err != nil {
			r.Err = err.Error()
		} else {
			r.OK = true
			r.Decoded = len(t.Bind)
			for i, want := range q.Want {
				if i < len(t.Bind) {
					got := t.Bind[i]
					r.RowOK[i] = got.Class == want.Class && got.Dest == want.Dest &&
						slices.Equal(got.Positions, want.Positions) && slices.Equal(got.Methods, want.Methods) &&
						len(got.Scopes) == 0
				}
			}
		}
		if err := enc.Encode(r); err != nil {
			fmt.Fprintln(os.Stderr, "atlasbind:", err)
			os.Exit(2)
		}
	}
	if err := sc.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "atlasbind:", err)
		os.Exit(2)
	}
}
