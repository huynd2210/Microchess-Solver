// Transposition-aware BFS probe for 4x5 microchess.
// Counts UNIQUE reachable positions per ply (dedup by 64-bit Zobrist key),
// giving the effective state-space growth curve for a TT-based solver.
//
// Board: idx = rank*4 + file, rank 0 = white's back rank (a1=0 .. d5=19).
// Pieces: 0 empty; white P=1 N=2 B=3 R=4 Q=5 K=6; black = +8.
// Rules: castling (Ka1/Rd1 -> Kc1/Rb1, mirrored on rank 5), no double step,
// promotion on last rank to Q/R/B/N. Halfmove clock / repetition ignored.
#include <bits/stdc++.h>
#include <thread>
#include <atomic>
using namespace std;
typedef uint64_t u64;

static const int N = 20;

struct Pos { array<uint8_t,N> b; uint8_t wtm; uint8_t castl; };

// ---- Zobrist ----
static u64 zPiece[16][N], zSide, zCastl[4];
static u64 rng64(u64& s){ s += 0x9E3779B97F4A7C15ULL; u64 z=s; z=(z^(z>>30))*0xBF58476D1CE4E5B9ULL; z=(z^(z>>27))*0x94D049BB133111EBULL; return z^(z>>31); }
static void initZ(){ u64 s=0x12345678; for(int p=0;p<16;p++)for(int q=0;q<N;q++) zPiece[p][q]=rng64(s); zSide=rng64(s); for(int i=0;i<4;i++) zCastl[i]=rng64(s); }

// ---- geometry ----
static vector<int> knightT[N], kingT[N], raysR[N], raysB[N]; // rays have -1 terminators
static void initGeom(){
    auto inb=[](int r,int f){return r>=0&&r<5&&f>=0&&f<4;};
    for(int s=0;s<N;s++){
        int r=s/4, f=s%4;
        const pair<int,int> KN[]={{1,2},{2,1},{-1,2},{-2,1},{1,-2},{2,-1},{-1,-2},{-2,-1}};
        for(auto& d : KN) if(inb(r+d.first,f+d.second)) knightT[s].push_back((r+d.first)*4+f+d.second);
        const pair<int,int> KG[]={{1,0},{-1,0},{0,1},{0,-1},{1,1},{1,-1},{-1,1},{-1,-1}};
        for(auto& d : KG) if(inb(r+d.first,f+d.second)) kingT[s].push_back((r+d.first)*4+f+d.second);
        const pair<int,int> RD[]={{1,0},{-1,0},{0,1},{0,-1}};
        for(auto& d : RD){ int rr=r+d.first, ff=f+d.second;
            while(inb(rr,ff)){ raysR[s].push_back(rr*4+ff); rr+=d.first; ff+=d.second; }
            raysR[s].push_back(-1); }
        const pair<int,int> BD[]={{1,1},{1,-1},{-1,1},{-1,-1}};
        for(auto& d : BD){ int rr=r+d.first, ff=f+d.second;
            while(inb(rr,ff)){ raysB[s].push_back(rr*4+ff); rr+=d.first; ff+=d.second; }
            raysB[s].push_back(-1); }
    }
}

static inline bool isWhite(int p){ return p>=1&&p<=6; }
static inline bool isBlack(int p){ return p>=9&&p<=14; }

static bool attacked(const Pos& P, int sq, bool byWhite){
    const array<uint8_t,N>& b = P.b;
    int r=sq/4, f=sq%4;
    if(byWhite){ for(int df=-1; df<=1; df+=2){ int ff=f+df; if(ff>=0&&ff<4&&r>=1 && b[(r-1)*4+ff]==1) return true; } }
    else       { for(int df=-1; df<=1; df+=2){ int ff=f+df; if(ff>=0&&ff<4&&r<=3 && b[(r+1)*4+ff]==9) return true; } }
    int kn=byWhite?2:10, kg=byWhite?6:14, bi=byWhite?3:11, ro=byWhite?4:12, qu=byWhite?5:13;
    for(int t : knightT[sq]) if(b[t]==kn) return true;
    for(int t : kingT[sq])   if(b[t]==kg) return true;
    // sliders along precomputed rays (-1 terminates each ray)
    const vector<int>& rb = raysB[sq];
    for(size_t i=0;i<rb.size();){
        size_t j=i;
        while(j<rb.size() && rb[j]>=0){
            int p=b[rb[j]];
            if(p){ if(p==bi||p==qu) return true; break; }
            j++;
        }
        while(i<rb.size() && rb[i]>=0) i++;  // skip to end of segment even after early break
        i++;
    }
    const vector<int>& rr = raysR[sq];
    for(size_t i=0;i<rr.size();){
        size_t j=i;
        while(j<rr.size() && rr[j]>=0){
            int p=b[rr[j]];
            if(p){ if(p==ro||p==qu) return true; break; }
            j++;
        }
        while(i<rr.size() && rr[i]>=0) i++;
        i++;
    }
    return false;
}

struct Move { uint8_t from, to, promo, castle; }; // promo = white piece code or 0

static void makeMove(Pos& P, const Move& m);
static vector<Move> genMoves(const Pos& P);
static int kingSq(const Pos& P, bool white);
static bool attacked(const Pos& P, int sq, bool byWhite);

static int kingSq(const Pos& P, bool white){
    int k = white?6:14;
    for(int i=0;i<N;i++) if(P.b[i]==k) return i;
    return -1;
}

static u64 perft(const Pos& P, int d){
    if(d==0) return 1;
    u64 n=0;
    bool moverWhite = P.wtm!=0;
    for(const Move& m : genMoves(P)){
        Pos Q=P; makeMove(Q,m);
        int ks=kingSq(Q, moverWhite);
        if(ks>=0 && attacked(Q, ks, !moverWhite)) continue; // leaves own king in check
        n += perft(Q, d-1);
    }
    return n;
}

static void makeMove(Pos& P, const Move& m){
    int p = P.b[m.from];
    P.b[m.from]=0;
    P.b[m.to]= m.promo ? (P.wtm? m.promo : m.promo+8) : p;
    if(m.castle==1){ P.b[0]=0; P.b[3]=0; P.b[2]=6; P.b[1]=4; }
    if(m.castle==2){ P.b[16]=0; P.b[19]=0; P.b[18]=14; P.b[17]=12; }
    if(p==6 || m.from==3 || m.to==3)  P.castl &= (uint8_t)~1;
    if(m.from==0)                     P.castl &= (uint8_t)~1;
    if(p==14|| m.from==19|| m.to==19) P.castl &= (uint8_t)~2;
    if(m.from==16)                    P.castl &= (uint8_t)~2;
    P.wtm ^= 1;
}

static vector<Move> genMoves(const Pos& P){
    vector<Move> out;
    bool w = P.wtm!=0;
    for(int s=0;s<N;s++){
        int p=P.b[s];
        if(w ? !isWhite(p) : !isBlack(p)) continue;
        int t = w? p : p-8;
        if(t==1){ // pawn
            int dr = w? 1 : -1; int r=s/4, f=s%4; int ns=s+dr*4;
            if(P.b[ns]==0){
                int nr=ns/4;
                if(nr==4||nr==0){ for(int pr : {5,4,3,2}) out.push_back({(uint8_t)s,(uint8_t)ns,(uint8_t)pr,0}); }
                else out.push_back({(uint8_t)s,(uint8_t)ns,0,0});
            }
            for(int df=-1;df<=1;df+=2){
                int ff=f+df; if(ff<0||ff>3) continue;
                int ts=(r+dr)*4+ff; int q=P.b[ts];
                if(q && (w? isBlack(q) : isWhite(q))){
                    int nr=ts/4;
                    if(nr==4||nr==0){ for(int pr : {5,4,3,2}) out.push_back({(uint8_t)s,(uint8_t)ts,(uint8_t)pr,0}); }
                    else out.push_back({(uint8_t)s,(uint8_t)ts,0,0});
                }
            }
        } else if(t==2){ for(int d : knightT[s]){ int q=P.b[d]; if(q==0||(w?isBlack(q):isWhite(q))) out.push_back({(uint8_t)s,(uint8_t)d,0,0}); } }
        else if(t==6){ for(int d : kingT[s])  { int q=P.b[d]; if(q==0||(w?isBlack(q):isWhite(q))) out.push_back({(uint8_t)s,(uint8_t)d,0,0}); } }
        else {
            if(t==3||t==5){
                const vector<int>& rays = raysB[s];
                for(size_t i=0;i<rays.size();){
                    size_t j=i;
                    while(j<rays.size() && rays[j]>=0){
                        int d=rays[j], q=P.b[d];
                        if(q==0) out.push_back({(uint8_t)s,(uint8_t)d,0,0});
                        else { if(w?isBlack(q):isWhite(q)) out.push_back({(uint8_t)s,(uint8_t)d,0,0}); break; }
                        j++;
                    }
                    while(i<rays.size() && rays[i]>=0) i++;  // skip whole segment even after early break
                    i++;
                }
            }
            if(t==4||t==5){
                const vector<int>& rays = raysR[s];
                for(size_t i=0;i<rays.size();){
                    size_t j=i;
                    while(j<rays.size() && rays[j]>=0){
                        int d=rays[j], q=P.b[d];
                        if(q==0) out.push_back({(uint8_t)s,(uint8_t)d,0,0});
                        else { if(w?isBlack(q):isWhite(q)) out.push_back({(uint8_t)s,(uint8_t)d,0,0}); break; }
                        j++;
                    }
                    while(i<rays.size() && rays[i]>=0) i++;
                    i++;
                }
            }
        }
    }
    if(w && (P.castl&1) && P.b[0]==6 && P.b[3]==4 && P.b[1]==0 && P.b[2]==0
        && !attacked(P,0,false) && !attacked(P,1,false) && !attacked(P,2,false))
        out.push_back({0,2,0,1});
    if(!w && (P.castl&2) && P.b[16]==14 && P.b[19]==12 && P.b[17]==0 && P.b[18]==0
        && !attacked(P,16,true) && !attacked(P,17,true) && !attacked(P,18,true))
        out.push_back({16,18,0,2});
    return out;
}


// ---- added: UCI move strings, move application by name, divide ----
static string sqName(int s){ string r; r += char('a' + s%4); r += char('1' + s/4); return r; }
static string mvName(const Move& m){
    string r = sqName(m.from) + sqName(m.to);
    if(m.promo) r += "nbrq"[m.promo-2];   // 2=N 3=B 4=R 5=Q
    return r;
}
static bool legalAfter(const Pos& P, const Move& m, Pos& Q){
    Q = P; bool moverWhite = P.wtm!=0; makeMove(Q,m);
    int ks = kingSq(Q, moverWhite);
    return !(ks>=0 && attacked(Q, ks, !moverWhite));
}
static vector<Move> legalMoves(const Pos& P){
    vector<Move> out; Pos Q;
    for(const Move& m : genMoves(P)) if(legalAfter(P,m,Q)) out.push_back(m);
    return out;
}

static u64 key(const Pos& P){
    u64 h = P.wtm? zSide:0;
    for(int i=0;i<N;i++) h ^= zPiece[P.b[i]][i];
    h ^= zCastl[P.castl&3];
    return h;
}

// flat open-addressing set of u64 keys
struct HashSet {
    static constexpr u64 EMPTY = ~0ULL;
    vector<u64> slots; size_t cnt=0, mask=0;
    void init(size_t cap){ // cap must be power of two
        slots.assign(cap, EMPTY); mask=cap-1; cnt=0;
    }
    bool insert(u64 k){
        size_t i = k & mask;
        while(true){
            u64 v = slots[i];
            if(v==EMPTY){ slots[i]=k; cnt++; return true; }
            if(v==k) return false;
            i=(i+1)&mask;
        }
    }
};



// =====================================================================
// PI-owned independent retrograde solver, "white material vs bare king".
// Classes are chains: black is always a lone king, so every black capture
// drops to a smaller white multiset -- an acyclic DAG solved bottom-up.
// Purpose: a THIRD implementation to check the delegated solver against,
// and an honest us/position measurement in compiled code.
// Values are from the SIDE TO MOVE's point of view.
// =====================================================================
static int NTHREADS = 1;
// The value array is written concurrently. Plain uint8_t access would be a data
// race (UB) even though the update is monotone; relaxed atomics make it defined
// and compile to the same mov on x86.
static inline uint8_t vload(const vector<uint8_t>& v, size_t i){
    return __atomic_load_n(&v[i], __ATOMIC_RELAXED);
}
static inline void vstore(vector<uint8_t>& v, size_t i, uint8_t x){
    __atomic_store_n(&v[i], x, __ATOMIC_RELAXED);
}
template <class F> static void run_parallel(int nt, size_t n, F f){
    if(nt <= 1){ f(0, n); return; }
    vector<thread> ts; size_t chunk = (n + nt - 1) / nt;
    for(int t=0;t<nt;t++){
        size_t lo = (size_t)t*chunk, hi = min(n, lo+chunk);
        if(lo>=hi) break;
        ts.emplace_back(f, lo, hi);
    }
    for(auto& th : ts) th.join();
}
enum : uint8_t { V_UNKNOWN=0, V_WIN=1, V_LOSS=2, V_DRAW=3, V_ILLEGAL=4 };

struct Class {
    vector<int> wp;             // white piece codes besides the king, e.g. {4} for R
    vector<uint8_t> val;        // indexed by idx()
    int k;                      // total pieces incl. both kings
    size_t size;
};

// index: base-20 over piece squares, order = [WK, wp..., BK]
static size_t pow20(int k){ size_t r=1; for(int i=0;i<k;i++) r*=20; return r; }

static bool decodeIdx(const Class& C, size_t id, Pos& P){
    P.b.fill(0); P.castl=0;
    size_t x = id;
    P.wtm = (uint8_t)(x & 1); x >>= 1;
    int sqs[8];
    for(int i=0;i<C.k;i++){ sqs[i] = (int)(x % 20); x /= 20; }
    for(int i=0;i<C.k;i++) for(int j=i+1;j<C.k;j++) if(sqs[i]==sqs[j]) return false;
    P.b[sqs[0]] = 6;                                   // white king
    for(size_t i=0;i<C.wp.size();i++) P.b[sqs[1+i]] = (uint8_t)C.wp[i];
    P.b[sqs[C.k-1]] = 14;                              // black king
    // legal iff the side that just moved is NOT in check
    bool moverWhite = (P.wtm==0);                      // side that just moved
    int ks = kingSq(P, moverWhite);
    if(ks<0) return false;
    if(attacked(P, ks, !moverWhite)) return false;
    return true;
}

static size_t encodeIdx(const Class& C, const Pos& P){
    int sqs[8]; int n=0;
    int wk=-1, bk=-1; vector<int> others;
    for(int i=0;i<N;i++){
        int p=P.b[i];
        if(p==6) wk=i; else if(p==14) bk=i; else if(p) others.push_back(i);
    }
    // others must match C.wp in the SAME order -> sort by piece code then square
    vector<pair<int,int>> pc;
    for(int sq : others) pc.push_back({P.b[sq], sq});
    // match C.wp order
    sqs[n++] = wk;
    vector<bool> used(pc.size(), false);
    for(int want : C.wp){
        int found=-1;
        for(size_t j=0;j<pc.size();j++) if(!used[j] && pc[j].first==want){ used[j]=true; found=pc[j].second; break; }
        if(found<0) return SIZE_MAX;
        sqs[n++] = found;
    }
    sqs[n++] = bk;
    if(n != C.k) return SIZE_MAX;
    size_t id=0;
    for(int i=C.k-1;i>=0;i--) id = id*20 + (size_t)sqs[i];
    return id*2 + (P.wtm?1:0);
}

static string cname(const vector<int>& wp){
    string s="K"; for(int p : wp) s += "?PNBRQK"[p]; s += "vk"; return s;
}

int main(int argc, char** argv){
    initZ(); initGeom();
    Pos P0; P0.b.fill(0);
    P0.b[0]=6; P0.b[1]=3; P0.b[2]=2; P0.b[3]=4;
    P0.b[7]=1;
    P0.b[16]=14; P0.b[17]=11; P0.b[18]=10; P0.b[19]=12;
    P0.b[15]=9;
    P0.wtm=1; P0.castl=3;

    // usage: mgen divide <depth> [move ...]

    if(argc>1 && string(argv[1])=="retro"){
        // Solve the chain of "white material vs bare black king" classes,
        // bottom-up. Black's only escape from a class is capturing a white
        // piece, which lands in a strictly smaller class -- acyclic.
        // every subset of {N,B,R,Q}, ordered by size so subclasses come first
        int maxpc = argc>2 ? atoi(argv[2]) : 3;
        NTHREADS = argc>3 ? atoi(argv[3]) : 1;
        fprintf(stderr, "threads: %d\n", NTHREADS);
        vector<int> avail = {2,3,4,5};                // N B R Q
        vector<vector<int>> chain;
        for(int sz=0; sz<=maxpc; sz++)
            for(int mask=0; mask<16; mask++){
                vector<int> wp;
                for(int i=0;i<4;i++) if(mask>>i & 1) wp.push_back(avail[i]);
                if((int)wp.size()==sz) chain.push_back(wp);
            }
        map<vector<int>, Class> done;                 // key = SORTED white pieces
        for(auto wp : chain){
            Class C; C.wp = wp; C.k = 2 + (int)wp.size(); C.size = pow20(C.k)*2;
            C.val.assign(C.size, V_ILLEGAL);
            clock_t t0 = clock();
            auto w0 = chrono::steady_clock::now();
            // pass 1: legality + terminals  (parallel over disjoint index ranges)
            atomic<size_t> legal_a{0};
            auto pass1 = [&](size_t lo, size_t hi){
                size_t loc=0;
                for(size_t id=lo; id<hi; id++){
                    Pos P;
                    if(!decodeIdx(C, id, P)) continue;
                    loc++;
                    vector<Move> lm = legalMoves(P);
                    if(lm.empty()){
                        bool mw = P.wtm!=0;
                        int ks = kingSq(P, mw);
                        C.val[id] = (ks>=0 && attacked(P, ks, !mw)) ? V_LOSS : V_DRAW;
                    } else C.val[id] = V_UNKNOWN;
                }
                legal_a += loc;
            };
            run_parallel(NTHREADS, C.size, pass1);
            size_t legal = legal_a.load();
            // fixed point.  SAFETY ARGUMENT: the update is MONOTONE -- a slot
            // goes UNKNOWN -> WIN/LOSS and never changes again -- so concurrent
            // in-place updates converge to the same unique fixed point whatever
            // the interleaving. Reading a stale UNKNOWN only costs an extra
            // sweep. uint8_t stores are atomic on x86, so no locks are needed.
            int iters=0;
            while(true){
                iters++;
                atomic<bool> changed_a{false};
                auto sweep = [&](size_t lo, size_t hi) -> void {
                  bool loc=false;
                  for(size_t id=lo; id<hi; id++){
                    if(vload(C.val, id) != V_UNKNOWN) continue;
                    Pos P; decodeIdx(C, id, P);
                    bool anyLoss=false, allWin=true;
                    for(const Move& m : legalMoves(P)){
                        Pos Q; legalAfter(P, m, Q);
                        uint8_t v;
                        // did black capture a white piece? -> smaller class
                        int cap = P.b[m.to];
                        if(cap && isWhite(cap)){
                            vector<int> sub = C.wp;
                            sub.erase(find(sub.begin(), sub.end(), cap));
                            vector<int> key = sub; sort(key.begin(), key.end());
                            auto it = done.find(key);
                            if(it == done.end()){ fprintf(stderr, "MISSING SUBCLASS\n"); abort(); }
                            size_t qid = encodeIdx(it->second, Q);
                            v = (qid==SIZE_MAX) ? V_DRAW : vload(it->second.val, qid);
                        } else {
                            size_t qid = encodeIdx(C, Q);
                            v = (qid==SIZE_MAX) ? V_DRAW : vload(C.val, qid);
                        }
                        if(v==V_LOSS) anyLoss=true;
                        if(v!=V_WIN) allWin=false;
                    }
                    if(anyLoss){ vstore(C.val, id, V_WIN); loc=true; }
                    else if(allWin){ vstore(C.val, id, V_LOSS); loc=true; }
                  }
                  if(loc) changed_a.store(true, memory_order_relaxed);
                };
                run_parallel(NTHREADS, C.size, sweep);
                if(!changed_a.load()) break;
            }
            size_t w=0,l=0,d=0;
            for(size_t id=0; id<C.size; id++){
                if(C.val[id]==V_UNKNOWN) C.val[id]=V_DRAW;
                if(C.val[id]==V_WIN) w++; else if(C.val[id]==V_LOSS) l++;
                else if(C.val[id]==V_DRAW) d++;
            }
            u64 chk=1469598103934665603ULL;
            for(size_t id=0; id<C.size; id++){ chk ^= C.val[id]; chk *= 1099511628211ULL; }
            double secs = (double)(clock()-t0)/CLOCKS_PER_SEC;
            double wallsecs = chrono::duration<double>(chrono::steady_clock::now()-w0).count();
            printf("class %-8s positions %9zu win %9zu loss %9zu draw %9zu "
                   "slots %10zu iters %2d cpu %6.2fs wall %6.2fs chk %016llx\n",
                   cname(wp).c_str(), legal, w, l, d, C.size, iters, secs, wallsecs,
                   (unsigned long long)chk);
            fflush(stdout);
            vector<int> key = wp; sort(key.begin(), key.end());
            done[key] = std::move(C);
        }
        return 0;
    }
    if(argc>1 && string(argv[1])=="divide"){
        int d = atoi(argv[2]);
        Pos P = P0;
        for(int i=3;i<argc;i++){
            string want = argv[i]; bool found=false; Pos Q;
            for(const Move& m : legalMoves(P)) if(mvName(m)==want){ legalAfter(P,m,Q); P=Q; found=true; break; }
            if(!found){ printf("ILLEGAL %s\n", want.c_str()); return 2; }
        }
        u64 tot=0;
        vector<pair<string,u64>> rows;
        for(const Move& m : legalMoves(P)){
            Pos Q; legalAfter(P,m,Q);
            u64 n = perft(Q, d-1);
            rows.push_back({mvName(m), n}); tot+=n;
        }
        sort(rows.begin(), rows.end());
        for(auto& r : rows) printf("%s: %llu\n", r.first.c_str(), (unsigned long long)r.second);
        printf("Nodes searched: %llu\n", (unsigned long long)tot);
        return 0;
    }

    printf("start moves: %zu\n", genMoves(P0).size());
    if(argc>1 && string(argv[1])=="perft"){
        int dmax = argc>2? atoi(argv[2]) : 6;
        for(int d=1; d<=dmax; d++) printf("perft %d = %llu\n", d, (unsigned long long)perft(P0,d));
        return 0;
    }

    if(argc>1 && string(argv[1])=="bfs"){
        int D = argc>2? atoi(argv[2]) : 40;
        size_t capBits = argc>3? (size_t)atoi(argv[3]) : 28;   // 2^28 slots = 2GB
        HashSet seen; seen.init(1ULL<<capBits);
        vector<Pos> frontier{P0};
        seen.insert(key(P0));
        printf("%3s %16s %18s %10s\n","ply","new_this_ply","cumulative_unique","sec");
        printf("%3d %16d %18zu %10.1f\n",0,1,seen.cnt,0.0);
        clock_t t0=clock();
        for(int d=1; d<=D; d++){
            vector<Pos> nf;
            for(const Pos& P : frontier){
                bool moverWhite = P.wtm!=0;
                for(const Move& m : genMoves(P)){
                    Pos Q=P; makeMove(Q,m);
                    int ks=kingSq(Q, moverWhite);
                    if(ks>=0 && attacked(Q, ks, !moverWhite)) continue;
                    if(seen.insert(key(Q))) nf.push_back(Q);
                }
            }
            frontier.swap(nf);
            printf("%3d %16zu %18zu %10.1f\n", d, frontier.size(), seen.cnt,
                   (double)(clock()-t0)/CLOCKS_PER_SEC);
            fflush(stdout);
            if(frontier.empty()){ printf("frontier exhausted -- FULL STATE SPACE ENUMERATED\n"); break; }
            if(seen.cnt > (1ULL<<capBits)*3/5){ printf("STOP: hash set >60%% full\n"); break; }
        }
        printf("total unique positions reached: %zu\n", seen.cnt);
        return 0;
    }
    if(argc>1 && string(argv[1])=="dump"){
        int D = argc>2? atoi(argv[2]) : 4;
        vector<Pos> frontier{P0};
        HashSet nxtd;
        for(int d=1; d<=D; d++){
            nxtd.init(1ULL<<22);
            vector<Pos> nf;
            for(const Pos& P : frontier) for(const Move& m : genMoves(P)){
                Pos Q=P; makeMove(Q,m);
                int ks=kingSq(Q, P.wtm!=0);
                if(ks>=0 && attacked(Q, ks, P.wtm==0)) continue;
                if(nxtd.insert(key(Q))) nf.push_back(Q);
            }
            frontier.swap(nf);
        }
        const char* FL="abcd";
        for(const Pos& P : frontier){
            string fen="";
            for(int r=4;r>=0;r--){
                int empty=0;
                for(int f=0;f<4;f++){
                    int p=P.b[r*4+f];
                    if(!p) empty++;
                    else { if(empty){fen+=char('0'+empty);empty=0;}
                           char c = p>8 ? "pnbrqk"[p-9] : (char)toupper("pnbrqk"[p-1]); fen+=c; }
                }
                if(empty) fen+=char('0'+empty);
                if(r) fen+='/';
            }
            string cr = (P.castl&1)? "K":""; if(P.castl&2) cr+="k";
            if(cr.empty()) cr="-";
            // count legal moves
            size_t legal=0;
            for(const Move& m : genMoves(P)){
                Pos Q=P; makeMove(Q,m);
                int ks=kingSq(Q, P.wtm!=0);
                if(ks>=0 && attacked(Q, ks, P.wtm==0)) continue;
                legal++;
            }
            printf("%s %s %s - 0 1 ;legal=%zu\n", fen.c_str(), P.wtm?"w":"b", cr.c_str(), legal);
        }
        return 0;
    }
    if(argc>3 && string(argv[3])=="debug"){
        const char* F="abcd";
        for(const Move& m : genMoves(P0))
            printf("  %c%d%c%d%s\n", F[m.from%4], m.from/4+1, F[m.to%4], m.to/4+1, m.castle?" (castle)":"");
        for(int r=4;r>=0;r--){ printf("  ");
            for(int f=0;f<4;f++) printf("%c ", P0.b[r*4+f]==0?'.':(P0.b[r*4+f]>8?"PNBRQK"[P0.b[r*4+f]-9]:"pnbrqk"[P0.b[r*4+f]-1]));
            printf("\n"); }
        return 0;
    }

    size_t capBytes = argc>1 ? atoll(argv[1])*(size_t)1000000 : (size_t)6000000000ULL; // budget in bytes
    int maxDepth = argc>2 ? atoi(argv[2]) : 20;

    vector<Pos> frontier; frontier.push_back(P0);
    double t0 = chrono::duration<double>(chrono::steady_clock::now().time_since_epoch()).count();
    u64 totalGen = 0;
    bool stopped = false;
    for(int d=1; d<=maxDepth && !stopped; d++){
        HashSet nxt;
        size_t est = frontier.size()*10 + 1024;
        size_t cap = 1<<12; while(cap < est) cap <<= 1;
        // memory guard: entries*8 bytes must fit budget alongside frontier storage (~24B/pos)
        size_t maxCap = capBytes/8;
        if(cap > maxCap){ cap = maxCap; }
        nxt.init(cap);
        vector<Pos> nf;
        bool overflow=false;
        for(const Pos& P : frontier){
            for(const Move& m : genMoves(P)){
                totalGen++;
                Pos Q=P; makeMove(Q,m);
                int ks=kingSq(Q, P.wtm!=0);
                if(ks>=0 && attacked(Q, ks, P.wtm==0)) continue; // illegal: own king in check
                if(nxt.cnt >= nxt.slots.size()*9/10){ overflow=true; break; }
                if(nxt.insert(key(Q))) nf.push_back(Q);
            }
            if(overflow) break;
        }
        double t1 = chrono::duration<double>(chrono::steady_clock::now().time_since_epoch()).count();
        if(overflow){
            printf("depth %2d: OVERFLOW (set full at %zu) - stopping\n", d, nxt.cnt);
            stopped=true;
        } else {
            frontier.swap(nf);
            printf("depth %2d: unique %10zu  (avg %.1f Mpos/s cum., elapsed %.1fs)\n",
                   d, frontier.size(), totalGen/1e6/(t1-t0), t1-t0);
        }
        fflush(stdout);
    }
    return 0;
}
