/* Freestanding static Linux x86-64 test worker. No libc, shell, or loader. */
typedef unsigned long u64;
static long call(long n,long a,long b,long c) { long r; __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b),"d"(c):"rcx","r11","memory"); return r; }
static long call6(long n,long a,long b,long c,long d,long e,long f) {
  register long fourth __asm__("r10")=d;
  register long fifth __asm__("r8")=e;
  register long sixth __asm__("r9")=f;
  long r;__asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b),"d"(c),"r"(fourth),"r"(fifth),"r"(sixth):"rcx","r11","memory");return r;
}
static void finish(long code) { call(60,code,0,0); for(;;){} }
static void read_all(unsigned char *p,u64 n) { while(n) {long r=call(0,0,(long)p,(long)n);if(r<=0)finish(3);p+=r;n-=r;} }
static void write_all(unsigned char *p,u64 n) { while(n) {long r=call(1,1,(long)p,(long)n);if(r<=0)finish(4);p+=r;n-=r;} }
static u64 be(unsigned char *p) {return ((u64)p[0]<<24)|((u64)p[1]<<16)|((u64)p[2]<<8)|p[3];}
static void copy(unsigned char *to,unsigned char *from,u64 n) {for(u64 i=0;i<n;i++)to[i]=from[i];}
static void pause_forever(void) {long ts[2]={1,0};for(;;)call(35,(long)ts,0,0);}
void _start(void) {
  unsigned char start[60],scratch[512],input[64],frame[61];read_all(start,60);
  u64 metadata=be(start+44),length=be(start+48),checkpoint=be(start+52);
  while(metadata){u64 n=metadata>512?512:metadata;read_all(scratch,n);metadata-=n;}
  if(length>64||checkpoint>64)finish(5);read_all(input,length);if(checkpoint)read_all(scratch,checkpoint);
  unsigned char mode=length?input[0]:0;
  if(mode==1)for(;;)__asm__ volatile("");
  if(mode==2){call(57,0,0,0);finish(6);}
  if(mode==3){call(257,-100,(long)"/etc/passwd",0);finish(6);}
  if(mode==4){call(41,2,1,0);finish(6);}
  if(mode==7)finish(0);
  if(mode==9)pause_forever();
  if(mode==10){
    /* Ordinary anonymous memory works; RLIMIT_AS rejects a reservation above
       the configured 64 MiB even without touching pages or supervisor polls. */
    long small=call6(9,0,1024*1024,3,0x22,-1,0);
    if(small<0)finish(80);
    ((volatile unsigned char*)small)[0]=1;
    ((volatile unsigned char*)small)[1024*1024-1]=2;
    if(call(11,small,1024*1024,0)!=0)finish(81);
    long large=call6(9,0,128*1024*1024,3,0x22,-1,0);
    if(large!=-12)finish(82); /* exact Linux ENOMEM, not a seccomp kill */
  }
  if(mode==11){u64 limit[2]={~0UL,~0UL};call(160,9,(long)limit,0);finish(83);}
  if(mode==13){call6(56,17,0,0,0,0,0);finish(83);}
  if(mode==14){call(62,call(39,0,0,0),0,0);finish(83);}
  if(mode==15){call(101,0,0,0);finish(83);}
  if(mode==16){u64 limit[2]={~0UL,~0UL};call6(302,0,9,(long)limit,0,0,0);finish(83);}
  if(mode==17){call(0,3,(long)scratch,1);finish(83);}
  if(mode==18){call(1,0,(long)scratch,1);finish(83);}
  copy(frame,(unsigned char*)"PNLOBS01",8);copy(frame+8,start+8,36);
  frame[44]=mode==5?2:1;frame[45]=0;frame[46]=0;frame[47]=0;frame[48]=8;
  frame[49]=0;frame[50]=0;frame[51]=0;frame[52]=1;
  u64 sum=0;for(u64 i=1;i<length;i++)sum+=input[i];for(u64 i=0;i<8;i++)frame[53+i]=(sum>>(8*i))&255;
  if(mode==6)frame[8]^=1;
  if(mode==12){frame[48]=65;write_all(frame,53);pause_forever();}
  write_all(frame,61);
  if(mode==5){unsigned char ack[40];read_all(ack,40);if(ack[0]!='P'||ack[3]!='A')finish(9);frame[44]=1;write_all(frame,61);}
  finish(0);
}
