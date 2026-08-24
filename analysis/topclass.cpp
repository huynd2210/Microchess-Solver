// EXACT reachable count for the largest material class (KBNPRvKBNPR):
// every position reachable from the start WITHOUT any capture.
//
// Why this class and why it is exactly measurable:
//   * no capture has happened, so both pawns are stuck on the d-file (a pawn
//     leaves its file only by capturing) and both bishops are on their starting
//     square colour;
//   * that collapses the index from 670,442,572,800 placements to 1,349,187,840,
//     which is a 1.35 GB bitmap -- it fits in RAM, so the count is EXACT, not
//     sampled and not extrapolated.
// This is the class the whole "reachability is ~0.008% of the index space"
// estimate rests on, so it is the one number worth measuring exactly.
#include <bits/stdc++.h>
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


// ---- perfect index for the no-capture class ----------------------------
static const int D2=7, D3=11, D4=15;
static int PCFG[3][2] = {{D2,D4},{D3,D4},{D2,D3}};
static inline int colr(int s){ return ((s/4)+(s%4))&1; }
struct Cfg { vector<int> freeSq, c1; int n1; u64 perBish, base; };
static Cfg CFG[3]; static u64 TOTAL_PLACE=0;
static u64 fallfac(int n,int k){ u64 r=1; for(int i=0;i<k;i++) r*=(n-i); return r; }
static void initIndex(){
    u64 base=0;
    for(int c=0;c<3;c++){
        Cfg& g=CFG[c]; g.freeSq.clear(); g.c1.clear();
        for(int s=0;s<N;s++) if(s!=PCFG[c][0] && s!=PCFG[c][1]) g.freeSq.push_back(s);
        for(int s : g.freeSq) if(colr(s)==colr(1)) g.c1.push_back(s);   // b1 colour
        g.n1=(int)g.c1.size();
        g.perBish = fallfac(16,6);
        g.base = base;
        base += (u64)g.n1*(g.n1-1)*g.perBish;
    }
    TOTAL_PLACE = base;
}
// rank a position; returns UINT64_MAX if it is not in this class
static u64 rankPos(const Pos& P){
    int wp=-1,bp=-1,wb=-1,bb=-1; int others[6]; int no=0;
    int wk=-1,wn=-1,wr=-1,bk=-1,bn=-1,br=-1;
    for(int s=0;s<N;s++){
        switch(P.b[s]){
            case 1: wp=s; break;  case 9:  bp=s; break;
            case 3: wb=s; break;  case 11: bb=s; break;
            case 6: wk=s; break;  case 14: bk=s; break;
            case 2: wn=s; break;  case 10: bn=s; break;
            case 4: wr=s; break;  case 12: br=s; break;
            default: break;
        }
    }
    if(wp<0||bp<0||wb<0||bb<0||wk<0||wn<0||wr<0||bk<0||bn<0||br<0) return UINT64_MAX;
    int c=-1; for(int i=0;i<3;i++) if(PCFG[i][0]==wp && PCFG[i][1]==bp) c=i;
    if(c<0) return UINT64_MAX;
    Cfg& g=CFG[c];
    int i1=-1,i2=-1;
    for(int i=0;i<g.n1;i++){ if(g.c1[i]==wb) i1=i; if(g.c1[i]==bb) i2=i; }
    if(i1<0||i2<0) return UINT64_MAX;
    int j = i2 - (i2>i1 ? 1 : 0);
    u64 rb = (u64)i1*(g.n1-1) + j;
    // remaining 16 squares, in ascending order
    int rem[16]; int nr=0;
    for(int s : g.freeSq) if(s!=wb && s!=bb) rem[nr++]=s;
    int order[6] = {wk,wn,wr,bk,bn,br};
    u64 rp=0; int cnt=nr;
    for(int t=0;t<6;t++){
        int pos=-1;
        for(int i=0;i<cnt;i++) if(rem[i]==order[t]){ pos=i; break; }
        if(pos<0) return UINT64_MAX;
        rp = rp*cnt + pos;
        for(int i=pos;i<cnt-1;i++) rem[i]=rem[i+1];
        cnt--;
    }
    return g.base + rb*g.perBish + rp;
}
static inline u64 slotOf(const Pos& P){
    u64 r = rankPos(P);
    if(r==UINT64_MAX) return UINT64_MAX;
    return (r*2 + (P.wtm?1:0))*4 + (P.castl&3);
}

int main(int argc,char** argv){
    initGeom(); initIndex();
    u64 slots = TOTAL_PLACE*8;
    printf("no-capture class: %llu placements, %llu slots (x2 stm x4 castling)\n",
           (unsigned long long)TOTAL_PLACE, (unsigned long long)slots);
    printf("bitmap: %.2f GB\n", (double)slots/8.0/1e9);
    fflush(stdout);
    vector<uint8_t> bits((size_t)(slots/8+1), 0);
    auto test=[&](u64 s){ return (bits[s>>3]>>(s&7))&1; };
    auto set_ =[&](u64 s){ bits[s>>3] |= (uint8_t)(1u<<(s&7)); };

    Pos P0; P0.b.fill(0);
    P0.b[0]=6; P0.b[1]=3; P0.b[2]=2; P0.b[3]=4; P0.b[7]=1;
    P0.b[16]=14; P0.b[17]=11; P0.b[18]=10; P0.b[19]=12; P0.b[15]=9;
    P0.wtm=1; P0.castl=3;
    u64 s0=slotOf(P0);
    if(s0==UINT64_MAX){ printf("ERROR: start position does not rank\n"); return 2; }
    set_(s0);
    vector<Pos> frontier{P0}; u64 total=1;
    printf("%4s %14s %16s %9s\n","ply","new","cumulative","sec");
    printf("%4d %14d %16llu %9.1f\n",0,1,(unsigned long long)total,0.0);
    clock_t t0=clock();
    for(int d=1; d<=200 && !frontier.empty(); d++){
        vector<Pos> nf;
        for(const Pos& P : frontier){
            bool mw = P.wtm!=0;
            for(const Move& m : genMoves(P)){
                if(P.b[m.to]!=0) continue;          // captures leave the class
                if(m.promo) continue;               // promotions leave the class
                Pos Q=P; makeMove(Q,m);
                int ks=kingSq(Q,mw);
                if(ks>=0 && attacked(Q,ks,!mw)) continue;
                u64 s=slotOf(Q);
                if(s==UINT64_MAX){ printf("ERROR: child does not rank\n"); return 3; }
                if(!test(s)){ set_(s); nf.push_back(Q); total++; }
            }
        }
        frontier.swap(nf);
        printf("%4d %14zu %16llu %9.1f\n", d, frontier.size(), (unsigned long long)total,
               (double)(clock()-t0)/CLOCKS_PER_SEC);
        fflush(stdout);
    }
    printf("\nEXACT reachable positions in the largest class: %llu\n",(unsigned long long)total);
    printf("  of %llu index slots           -> %.4f%% dense\n",
           (unsigned long long)slots, 100.0*total/(double)slots);
    printf("  of 670,442,572,800 x 8 naive  -> %.5f%% dense\n",
           100.0*total/(670442572800.0*8));
    return 0;
}
