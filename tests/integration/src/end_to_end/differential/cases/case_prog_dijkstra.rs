// Shortest paths (campaign 17, program 10): an O(n^2) Dijkstra over a
// 32-node directed graph whose adjacency matrix lives in `.rodata` (built
// by a `const fn` LCG, ~25% density plus a ring for connectivity) with
// input-derived edge weights, from an input-selected source, with a
// distance array, a visited set and a predecessor array on the stack; the
// path to an input-selected target is walked back through the
// predecessors, a second run uses the other input's weights and source,
// and the distance arrays, path hash, hop counts and the settled-node
// count are folded into the result.
const fn gen_adj() -> [[u8; 32]; 32] {
    let mut m = [[0u8; 32]; 32];
    let mut s: u32 = 0x1234_5678;
    let mut i = 0usize;
    while i < 32 {
        let mut j = 0usize;
        while j < 32 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if i != j && (s >> 24) % 4 == 0 {
                m[i][j] = 1 + ((s >> 16) & 15) as u8;
            }
            j += 1;
        }
        m[i][(i + 1) % 32] = 9;
        i += 1;
    }
    m
}

static ADJ: [[u8; 32]; 32] = gen_adj();

const INF: u32 = u32::MAX;

// Runs Dijkstra from `src`; edge (u, v) costs `ADJ[u][v] + ((salt >> (v &
// 31)) & 7)` when present. Returns the number of settled nodes.
fn dijkstra(src: usize, salt: u32, dist: &mut [u32; 32], prev: &mut [u8; 32]) -> u32 {
    let mut visited = [false; 32];
    let mut i = 0usize;
    while i < 32 {
        dist[i] = INF;
        prev[i] = 0xff;
        i += 1;
    }
    dist[src] = 0;
    let mut settled = 0u32;
    let mut round = 0usize;
    while round < 32 {
        // Pick the unvisited node with the smallest distance.
        let mut u = 32usize;
        let mut best = INF;
        i = 0;
        while i < 32 {
            if !visited[i] && dist[i] < best {
                best = dist[i];
                u = i;
            }
            i += 1;
        }
        if u == 32 {
            break;
        }
        visited[u] = true;
        settled += 1;
        let mut v = 0usize;
        while v < 32 {
            let w = ADJ[u][v];
            if w != 0 && !visited[v] {
                let cost = w as u32 + ((salt >> (v as u32 & 31)) & 7);
                let nd = best + cost;
                if nd < dist[v] {
                    dist[v] = nd;
                    prev[v] = u as u8;
                }
            }
            v += 1;
        }
        round += 1;
    }
    settled
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut dist = [0u32; 32];
    let mut prev = [0u8; 32];
    let src = (input1 % 32) as usize;
    let target = ((input1 >> 5) % 32) as usize;
    let settled = dijkstra(src, input2, &mut dist, &mut prev);
    // Walk the path back from the target.
    let mut path_hash = 0u32;
    let mut hops = 0u32;
    let mut cur = target;
    while cur != src && hops < 32 && prev[cur] != 0xff {
        path_hash = path_hash.rotate_left(5) ^ cur as u32;
        cur = prev[cur] as usize;
        hops += 1;
    }
    let reached = (cur == src) as u32;
    let mut dist2 = [0u32; 32];
    let mut prev2 = [0u8; 32];
    let src2 = (input2 % 32) as usize;
    let settled2 = dijkstra(src2, input1, &mut dist2, &mut prev2);
    let mut acc = settled << 8 ^ settled2 << 16 ^ hops << 24 ^ reached << 31 ^ path_hash;
    let mut i = 0usize;
    while i < 32 {
        acc = acc.rotate_left(3)
            ^ dist[i].wrapping_mul(0x9e37_79b9)
            ^ dist2[i]
            ^ (prev2[i] as u32) << 24;
        i += 1;
    }
    acc ^ dist[target]
}
