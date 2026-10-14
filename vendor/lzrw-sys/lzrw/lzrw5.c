/*
IMPORTANT NOTE: The LZRW5 document was posted on 23-Jul-1991.
That document appears at the end of this file.

At the time I thought LZRW5 was a new idea. However, I was wrong as is
shown by the following email which I have included here before the
original document.

======================================================================
Path: spam!ross
From: ross@spam.ua.oz.au (Ross Williams)
Newsgroups: comp.compression
Subject: LZRW5 is NOT original.
Summary: LZRW5 is NOT original.
Keywords: lzrw5 data compression algorithm orginality yabba yabbawhap
Message-ID: <967@spam.ua.oz>
Date: 28 Jul 91 16:21:46 GMT
Sender: ross@spam.ua.oz
Followup-To: comp.compression
Distribution: world
Organization: Statistics, Pure & Applied Mathematics, University of Adelaide
Lines: 26

LZRW5 is NOT Original
=====================
Earlier I posted my LZRW5 as an original algorithm. However, I have
recently discovered that it has appeared before. The same idea of
keeping an array of pointers to phrases in an LZW algorithm was used
by Dan Bernstein in his yabbawhap compression package which can be
found in comp.sources.unix vol 24. At least one site where you can get
this is UUNET (in Virginia) in

   uunet.uu.net:comp.sources.unix/volume24/yabbawhap.

Dan developed the yabba program about one year ago and posted it to
alt.comp.compression in March 1991.

Boo hoo,

Ross Williams
ross@spam.ua.oz.au

PS: How embarrassing - I actually created alt.comp.compression! Dan
posted yabba after I thought I had destroyed alt.comp.compression and
while I was busy with creating comp.compression! I nearly looked at
yabba recently too, but decided to spend the time getting LZRW4 and
LZRW5 out! I have not yet sighted yabba source code, being busy at
present with non-compression stuff.

======================================================================
Path: spam!sirius.ucs.adelaide.edu.au!yoyo.aarnet.edu.au!munnari.oz.au!samsung!uakari.primate.wisc.edu!sdd.hp.com!wupost!emory!ox.com!yale.edu!cmcl2!kramden.acf.nyu.edu!brnstnd
From: brnstnd@kramden.acf.nyu.edu (Dan Bernstein)
Newsgroups: comp.compression
Subject: Re: LZRW5: A Turbocharged LZW-like Decompressor.
Message-ID: <28577.Jul2519.12.1791@kramden.acf.nyu.edu>
Date: 25 Jul 91 19:12:17 GMT
References: <961@spam.ua.oz>
Organization: IR
Lines: 22

While I'm sure we all appreciate Ross's extensive contributions to the
field of compression, I have to point out that his ``turbocharging'' is
exactly the method stated in my introduction to Y coding (draft 4b,
3/19/91), in section 2 (``Implementing LZW''):

: --- Decoding
:
: LZW decoding is even easier than encoding: the dictionary does not have
: to support searching. The easiest (and generally fastest) method is to
: keep I in memory as it is reconstructed, and to keep track of which
: substring of I a given dictionary number corresponds to. To add pc to
: the dictionary means to add the pair (pointer to start of pc within I,
: length of pc) at the right spot in an array indexed by dictionary
: numbers. There are methods which take slightly less memory than this,
: but they are slower.

I applied this to LZW last year, and indeed it makes a very fast, simple
decompressor. My ``unwhap'' decompressor for AP, as included in my
yabbawhap package, comp.sources.unix volume 24, also applies the same
technique.

---Dan

======================================================================
Path: spam!sirius.ucs.adelaide.edu.au!yoyo.aarnet.edu.au!munnari.oz.au!samsung!mips!swrinde!zaphod.mps.ohio-state.edu!think.com!hsdndev!cmcl2!kramden.acf.nyu.edu!brnstnd
From: brnstnd@kramden.acf.nyu.edu (Dan Bernstein)
Newsgroups: comp.compression
Subject: Re: LZRW5 is NOT original.
Message-ID: <18214.Jul2902.59.2991@kramden.acf.nyu.edu>
Date: 29 Jul 91 02:59:29 GMT
References: <967@spam.ua.oz>
Organization: IR
Lines: 42

In article <967@spam.ua.oz> ross@spam.ua.oz.au (Ross Williams) writes:
> Earlier I posted my LZRW5 as an original algorithm. However, I have
> recently discovered that it has appeared before. The same idea of
> keeping an array of pointers to phrases in an LZW algorithm was used
> by Dan Bernstein in his yabbawhap compression package which can be
> found in comp.sources.unix vol 24.

I used it in unwhap, and it's the main reason unwhap runs as fast as
uncompress---AP does three to five times more dictionary manipulations
than LZW. However, I haven't figured out how to apply it to Y coding.
(I have a very promising lead, though, so don't be surprised if and when
undabba is faster than unyabba... more news soon...)

> Dan developed the yabba program about one year ago and posted it to
> alt.comp.compression in March 1991.

Actually, I posted my introduction to Y coding to alt.comp.compression
on March 6, 1991, a few days before the group was officially removed. I
included a slightly revised draft with yabbawhap, which I posted to
comp.sources.unix, not alt.comp.compression. I discovered Y coding on
December 26, 1990. A year ago (July 28, 1990, according to my mail
records) I discovered AP coding, which I called ``B coding'' in private
discussions until a month later, when Brad Templeton pointed out
Storer's prior discovery of the method. To get back to the topic at
hand, my implementations of AP during August 1990 all used the same
technique as in Ross's recent LZRW5. I don't know if I was the first to
come upon this idea.

While I'm on a historical accuracy kick: Don't trust anything you read
in the May 1991 Computer Language article on data compression. I quote:
``One of the most popular data-compression algorithms is the
sliding-window dictionary (SWD) variation on the Lempel-Ziv-Welch (LZW)
algorithm, developed around 1984-85.'' First of all, LZW was developed
in 1983 (both by Welch and by Miller-Wegman), and published in 1984, so
``developed around 1984-85'' is silly. Second, I haven't gone through
the code in that article in detail, but there is no such thing as ``the
sliding-window dictionary variation on LZW.'' The article appears to
describe one of the LZ77 variants. The article goes on to use ``SWD'' as
the name of the method; this is nonstandard at best (though one can say
the same of almost all of Storer's terminology).

---Dan

======================================================================

--<Start of LZRW5 paper>--

LZRW5: A TURBOCHARGED LZW-LIKE DECOMPRESSOR
===========================================
Author : Ross Williams
Date   : 23-Jul-1991.

This is a public domain document and may be copied freely so long as
it is not modified (but text formatting transformations are permitted
though).

Abstract
--------
A technique is described for constructing an LZW-like algorithm that
yields the same compression as LZW but whose decompressor runs much
faster (93K/s for a 68000 assembler implementation on an 8MHz
Mac-SE20). The implementation of the compression algorithm is open
ended (although a modified version of LZW could be used).
Unfortunately it is possible that the technique may be covered by
third party patents.

Distribution Algorithms
-----------------------
Over the last month or so, one or two people have posted requests for
what I call "distribution algorithms", which are data compression
algorithms where the decompressor's speed and memory consumption is
much more important than the compressor's. Such algorithms are useful
when distributing data, where the data will be compressed once by the
distributor, but decompressed thousands or perhaps millions of times
by users. The distributor can afford to use a hugely powerful computer
to perform the compression, but the decompression must be performed on
each user computer using minimum resources.

While thinking about this, I happened to apply the phrase table idea
of my LZRW2 algorithm to the LZW algorithm to produce an LZW-like
algorithm that provides the same compression performance as LZW, but
yields very much faster decompression, possibly at a cost in memory
consumption and speed of the compressor.

Description of the LZRW5 Idea
------------------------------
The trick is for the compressor and decompressor to maintain a "phrase
table" of roughly the most recent 2^n phrases parsed (where n is the
codeword width). Apart from a large memory output buffer, this is the
ONLY data structure of the decompressor. The compressor is likely to
need other data structures. Each phrase table entry consists of a
pointer to the phrase and the length of the phrase. An index rotates
through this table indicating the next position to overwrite. Pointers
point to the input buffer in the compressor and the output buffer in
the decompressor.

To process each phrase, the decompressor receives a phrase number from
the compressor. It then looks up the pointer and the length of the phrase
and copies the phrase to the output. It then overwrites the next position
in the phrase table with a pointer to the phrase just coded, but puts
in the length+1 in the table.

The compressor does all this too but keeps an additional table that
enables it to select the longest match from the strings in the phrase
table.

Here is a rough outline for the decompressor code. A complete 68000
C and assembly language version appears in Appendix A.

   unsigned cycle;              -- Index of next slot to write.
   unsigned word *p_src;        -- Pointer to input block.
   unsigned byte *p_dst;        -- Pointer to output block.
   char *pointer[65536];        -- Stores pointers to phrases.
   unsigned *length[65536];     -- Stores length of phrases.

   p_src=pointer to start of input  block.
   p_dst=pointer to start of output block.
   cycle=256;
   forall ch in 0..255, pointer[ch]=&<constant byte being value of ch>;
   forall ch in 0..255, length[ch]=1;

   while (p_src!=end of input block) loop
      unsigned phrasenum,len;
      phrasenum=*p_src++;       -- Pick up next code word.
      p=pointer[phrasenum];     -- Get pointer and length of phrase to copy
      len=length[phrasenum];
      pointer[cycle]=p_dst;     -- Add a new phrase one byte longer.
      length[cycle]=len+1;
      cycle++;                  -- Update cycle counter.
      if (cycle==2^n) cycle=256;-- Note:0..255 reserved for alphabet.
      while (len--) *p_dst++=*p++; -- Copy phrase to output.
   end loop

This decompressor code has the full power of LZW while giving roughly
the speed of the fast LZ77 decompressors (e.g. the A1 or LZRW1
algorithms). For 65536 phrases, it will use 320K, plus space for the
output buffer. The algorithm is adaptive so reducing the table size
will probably not impact on compression grossly (you may or may not
want to pack the resultant narrower codewords).

This decompressor avoids the nasty problem LZW has with adding a
phrase and then using it immediately (and having to look up the first
character etc). The problem is solved in a LZ77 kind of a way by using
pointers to the output to do automatic overlapping! This decompressor
is also adaptive because it overwrites old phrases with new ones. Unix
compress has to do its adaption explicitly.

There are any number of compressor implementations that could operate
in conjunction with the decompressor above. The compressor can use
expensive data structures to find the best match. It would also be a
very easy and efficient thing to modify the decompressor to (say) add
TWO strings to the dictionary instead of one (e.g. strings of length 1
and 3 characters longer, or this and the next phrase) (many other
similar variations are possible).

There is no reason why a conventional LZW hash table compressor could
not be modified to drive the output through a phrase table. This would
yield roughly the same compression speed.

The code below is an early draft of the LZRW5 algorithm. The
compressor uses binary trees and is rather pathetic (it runs at about
1k/s) as I have been concentrating on the deecompressor.

The 68000 assembly language version of the decompressor runs at
93K/sec on my Mac-SE20 (43K/sec in C). This (the 93K/sec) is only 2/3
the speed of LZRW1 (which yields worse compression), but a lot faster
than Unix compress. The compressor below runs at about 1K/second.

So far I have been unable to ascertain whether this idea is covered by
any patents. If anyone can help here, I would be most grateful.

Enjoy,

Ross Williams
ross@spam.ua.oz.au


Appendix A: Half-Baked Code for LZRW5
-------------------------------------
The following code is public domain and is provided "as is". As most
experimental code, it is poorly designed, structured, coded, and
commented.

WARNING: It is possible that this code may be subject to patent
protection by third parties. As such, it should not be included in
production software or executed for any but experimental purposes
without prior legal advice.

WARNING: This code will not work properly for more than 65536 phrases.
You can be sure of not reaching this limit by only feeding in input
files of at most length 65536. The program does not contain an
assertion for this condition.

NOTE: This code is supplied more for exposition than execution.

/******************************************************************************/
/*                                                                            */
/*                                    LZRW5.C                                 */
/*                                                                            */
/******************************************************************************/
/*                                                                            */
/* Author  : Ross Williams.                                                   */
/* Date    : 17-Jul-1991.                                                     */
/* Release : 1.                                                               */
/*                                                                            */
/******************************************************************************/

                            /* INCLUDE FILES                                  */
                            /* =============                                  */
#include "lzrw.h"
#include <stdio.h>
#include <stdlib.h>
#include "assert.h"

/******************************************************************************/

/* When I first started to get concerned about the portability of my C code,  */
/* I switched over to using only macro defined types UBYTE, UWORD, ULONG and  */
/* one or two others. While, these are useful for most purposes, they impair  */
/* efficiency as, if I have a variable whose range will be [0,1000], I will   */
/* declare it as a UWORD. This will translate into (say) "short int" and      */
/* hence may be less efficient than just an "int" which represents the        */
/* natural size of the machine. Before releasing LZRW3-A, I realized this     */
/* mistake. Unfortunately, I can't access the ftp archive with my portability */
/* header in it in time for this algorithm's release and so I am including an */
/* extra definition. The definition UCARD stands for an unsigned (cardinal)   */
/* type that can hold values in the range [0,32767]. This is within the ANSI  */
/* range of a standard int or unsigned. No assumption about overflow of this  */
/* type is made in the code (i.e. all usages are within range and I do not    */
/* use the value -1 to detect the end of loops.).                             */
/* You can use either "unsigned" or just "int" here depending on which is     */
/* more efficient in your environment (both the same probably).               */
#define UCARD unsigned

struct node_t
  {
   UBYTE *p_phrase;   /*  4 bytes. Pointer to phrase represented by the node. */
   UBYTE len_phrase;  /*  1 byte.  Length of the phrase.                      */
   /* UBYTE same;          1 byte.  Length of prefix shared by parent node.    */
   /* UWORD parent;        2 bytes. Phrase number of parent phrase.            */
   UWORD left;        /*  2 bytes. Phrase number of left child phrase.        */
   UWORD right;       /*  2 bytes. Phrase number of right child phrase.       */
                      /* --------                                             */
                      /* 12 bytes is the ideal (no padding) structure length. */
  };

#define MAX_NODES         (1L<<16)
#define MAX_PHRASE_LENGTH (255L)
#define ALPHABET_SIZE     (256L)

#define TEST_ARRAY_ITEMS 4
typedef struct node_t test_node_array[TEST_ARRAY_ITEMS];

#define U(X)            ((ULONG) X)
#define SIZE_P_BYTE     (U(sizeof(UBYTE *)))
#define ALIGNMENT_FUDGE (U(64L))

/* Calculate the memory required by the compressor. */
#define MEM_NODE_TABLE (MAX_NODES*(sizeof(test_node_array)/TEST_ARRAY_ITEMS))
#define MEM_ALPHA_TABLE  (256L)
#define MEM_COMPRESS     (MEM_NODE_TABLE   + \
                          MEM_ALPHA_TABLE  + \
                          ALIGNMENT_FUDGE)

/* Calculate the memory required by the decompressor. */
#define MEM_PHRASE_TABLE (MAX_NODES*SIZE_P_BYTE)
#define MEM_LENGTH_TABLE (MAX_NODES*1L)
#define MEM_DECOMPRESS   (MEM_PHRASE_TABLE + \
                          MEM_LENGTH_TABLE + \
                          MEM_ALPHA_TABLE  + \
                          ALIGNMENT_FUDGE)

#define MEM_REQ (MEM_COMPRESS>MEM_DECOMPRESS?MEM_COMPRESS:MEM_DECOMPRESS)

static struct compress_identity identity =
{
 U(0x4A619944),                           /* Algorithm identification number. */
 MEM_REQ,                                 /* Working memory (bytes) required. */
 "LZRW5",                                 /* Name of algorithm.               */
 "1.0",                                   /* Version number of algorithm.     */
 "17-Jul-1990",                           /* Date of algorithm.               */
 "Public Domain",                         /* Copyright notice.                */
 "Ross N. Williams",                      /* Author of algorithm.             */
 "Renaissance Software",                  /* Affiliation of author.           */
 "Public Domain"                          /* Vendor of algorithm.             */
};

LOCAL int compare_strings(UBYTE *,UCARD,UBYTE *,UCARD);
LOCAL void compress_compress  (UBYTE *,UBYTE *,ULONG,UBYTE *,ULONG *);
LOCAL void compress_decompress(UBYTE *,UBYTE *,ULONG,UBYTE *,ULONG *);

/******************************************************************************/

/* This function is the only function exported by this module.                */
/* Depending on its first parameter, the function can be requested to         */
/* compress a block of memory, decompress a block of memory, or to identify   */
/* itself. For more information, see the specification file "compress.h".     */

EXPORT void lzrw5_compress(action,wrk_mem,src_adr,src_len,dst_adr,p_dst_len)
UWORD     action;      /* Action to be performed.                             */
UBYTE   *wrk_mem;      /* Address of working memory we can use.               */
UBYTE   *src_adr;      /* Address of input data.                              */
ULONG    src_len;      /* Length  of input data.                              */
UBYTE   *dst_adr;      /* Address to put output data.                         */
ULONG *p_dst_len;      /* Address of longword for length of output data.      */
{
 switch (action)
   {
    case COMPRESS_ACTION_IDENTITY:
       *(uintptr_t *)p_dst_len = (uintptr_t)&identity;
       break;
    case COMPRESS_ACTION_COMPRESS:
       compress_compress(wrk_mem,src_adr,src_len,dst_adr,p_dst_len);
       break;
    case COMPRESS_ACTION_DECOMPRESS:
       compress_decompress(wrk_mem,src_adr,src_len,dst_adr,p_dst_len);
       break;
   }
}

/******************************************************************************/

/* The following #define defines the length of the copy flag that appears at  */
/* the start of the compressed file. The value of four bytes was chosen       */
/* because the fast_copy routine on my Macintosh runs faster if the source    */
/* and destination blocks are relatively longword aligned.                    */
/* The actual flag data appears in the first byte. The rest are zeroed so as  */
/* to normalize the compressed representation (i.e. not non-deterministic).   */
#define FLAG_BYTES 4

/* The following #defines define the meaning of the values of the copy        */
/* flag at the start of the compressed file.                                  */
#define FLAG_COMPRESS 0     /* Signals that output was result of compression. */
#define FLAG_COPY     1     /* Signals that output was simply copied over.    */

/* The 68000 microprocessor (on which this algorithm was originally developed */
/* is fussy about non-aligned arrays of words. To avoid these problems the    */
/* following macro can be used to "waste" from 0 to 3 bytes so as to align    */
/* the argument pointer.                                                      */
#define ULONG_ALIGN_UP(X) (X)

/******************************************************************************/

LOCAL int compare_strings(p_s1,len1,p_s2,len2)
/* Compares the two strings lexicographically and returns:                    */
/*    -1 if s1<s2                                                             */
/*     0 if s1==s2                                                            */
/*     1 if s1>s2                                                             */
register UBYTE *p_s1,*p_s2;  /* Pointers to first byte of each string. */
         UCARD len1,len2;    /* Number of bytes in each string.        */
{
 register UCARD minlen = len1<len2 ? len1 : len2;
 register UCARD i;
 for (i=0;i<minlen;i++)
    if (*p_s1++!=*p_s2++)
       if (*--p_s1<*--p_s2)
          return -1;
       else
          return 1;
 if (len1<len2) return -1;
 if (len1>len2) return  1;
 return 0;
}


LOCAL void compress_compress
           (p_wrk_mem,p_src_first,src_len,p_dst_first,p_dst_len)
/* Input  : Hand over the required amount of working memory in p_wrk_mem.     */
/* Input  : Specify input block using p_src_first and src_len.                */
/* Input  : Point p_dst_first to the start of the output zone (OZ).           */
/* Input  : Point p_dst_len to a ULONG to receive the output length.          */
/* Input  : Input block and output zone must not overlap.                     */
/* Output : Length of output block written to *p_dst_len.                     */
/* Output : Output block in Mem[p_dst_first..p_dst_first+*p_dst_len-1]. May   */
/* Output : write in OZ=Mem[p_dst_first..p_dst_first+src_len+MAX_CMP_GROUP-1].*/
/* Output : Upon completion guaranteed *p_dst_len<=src_len+FLAG_BYTES.        */
UBYTE *p_wrk_mem;
UBYTE *p_src_first;
ULONG  src_len;
UBYTE *p_dst_first;
ULONG *p_dst_len;
{
 /* p_src and p_dst step through the source and destination blocks.           */
 UBYTE *p_src = p_src_first;
 UBYTE *p_dst = p_dst_first;

 /* The following variables are never modified and are used in the            */
 /* calculations that determine when the main loop terminates.                */
 UBYTE *p_src_post  = p_src_first+src_len;
 UBYTE *p_dst_post  = p_dst_first+src_len;
 UBYTE *p_src_max_256 = p_src_first+src_len-256;

 struct node_t *node;
 UBYTE **pointer;
 UBYTE *length;
 UBYTE *alphabet;
 ULONG next;
 UCARD i;

 node     = (struct node_t *) ULONG_ALIGN_UP(p_wrk_mem);
 alphabet = (UBYTE *)  &node[MAX_NODES];

 /* Load the node table with nodes for the alphabet. */
 for (i=0;i<ALPHABET_SIZE;i++)
   {
    alphabet[i]=i;
    node[i].p_phrase=&alphabet[i];
    node[i].len_phrase=1;
    node[i].left=0;
    node[i].right=0;
   }
 next=256;


 /* To start, we write the flag bytes. Being optimistic, we set the flag to   */
 /* FLAG_COMPRESS. The remaining flag bytes are zeroed so as to keep the      */
 /* algorithm deterministic.                                                  */
 *p_dst++=FLAG_COMPRESS;
 {UCARD i; for (i=2;i<=FLAG_BYTES;i++) *p_dst++=0;}

 while (p_src<p_src_max_256)
   {/* Begin main processing loop. */
    UWORD current;
    UWORD bestnode;
    UCARD bestlen;
    int cmp;
    UBYTE *p_ziv;

    if (p_dst>p_dst_post) goto overrun;

    p_ziv=p_src;

    /* Run down the tree matching against the Ziv. */
    current=*p_ziv;
    bestnode=current;
    bestlen=1;
    while (TRUE)
      {
       UWORD t;
       struct node_t *pn=&node[current];
       cmp=compare_strings(p_ziv,bestlen+1,pn->p_phrase,pn->len_phrase);
       if (cmp==-1)
          {if ((t=pn->left)==0) break; else current=t;}
       else
          if (cmp==1)
             {if ((t=pn->right)==0) break; else current=t;}
          else
             {bestnode=current;
              if (++bestlen==MAX_PHRASE_LENGTH) break;}
      }

    /* Assert: bestnode is the longest match with the Ziv. */
    /* Assert: current is "append" leaf for the Ziv. Direction in cmp. */

    /* Check that node contains a string that matches Ziv */
/* DEBUG
    for (i=0;i<node[bestnode].len_phrase;i++)
       if (p_ziv[i]!=node[bestnode].p_phrase[i])
          error("Best phrase does not match Ziv!!!!");
*/

    /* Transmit the node number. */
    p_src+=node[bestnode].len_phrase;
    *p_dst++=bestnode>>8;               /* Big endian order for 68000. */
    *p_dst++=bestnode&0xFF;

    /* Delete the oldest phrase from the tree. */
    /* <<<Skipped in first version>>> */
    /* Note: must ignore nodes with a length of zero. */

    /* Insert a new phrase into the tree. */
    if (node[current].len_phrase==MAX_PHRASE_LENGTH)
       node[next].len_phrase=0;
    else
      {
       /* Create a new leaf node with the string in it. */
       struct node_t *pn=&node[next];
       pn->p_phrase=p_ziv;
       pn->len_phrase=node[bestnode].len_phrase+1;  /* Opt: Use a wrap for this later! */
       pn->left=0;
       pn->right=0;
       /* Connect it up to the append node. */
       if (cmp==-1)
          node[current].left=next;
       else
          node[current].right=next;
      }

    if (++next == MAX_NODES)
       goto overrun;

   } /* End main processing loop. */

 /* At this point there are 256 or less bytes to go. These are most easily */
 /* coded as one byte strings. */
 while (p_src!=p_src_post)
   {
    *p_dst++=0;
    *p_dst++=*p_src++;
   }

 /* Write the length of the output block to the dst_len parameter and return. */
 *p_dst_len=p_dst-p_dst_first;
 return;

 /* Jump here as soon as an overrun is detected. An overrun is defined to     */
 /* have occurred if p_dst>p_dst_first+src_len. That is, the moment the       */
 /* length of the output written so far exceeds the length of the input block.*/
 /* The algorithm checks for overruns at least at the end of each group       */
 /* which means that the maximum overrun is MAX_CMP_GROUP bytes.              */
 /* Once an overrun occurs, the only thing to do is to set the copy flag and  */
 /* copy the input over.                                                      */
 overrun:
 *p_dst_first=FLAG_COPY;
 fast_copy(p_src_first,p_dst_first+FLAG_BYTES,src_len);
 *p_dst_len=src_len+FLAG_BYTES;
}

/******************************************************************************/

LOCAL void compress_decompress
           (p_wrk_mem,p_src_first,src_len,p_dst_first,p_dst_len)
/* Input  : Hand over the required amount of working memory in p_wrk_mem.     */
/* Input  : Specify input block using p_src_first and src_len.                */
/* Input  : Point p_dst_first to the start of the output zone.                */
/* Input  : Point p_dst_len to a ULONG to receive the output length.          */
/* Input  : Input block and output zone must not overlap. User knows          */
/* Input  : upperbound on output block length from earlier compression.       */
/* Input  : In any case, maximum expansion possible is nine times.            */
/* Output : Length of output block written to *p_dst_len.                     */
/* Output : Output block in Mem[p_dst_first..p_dst_first+*p_dst_len-1].       */
/* Output : Writes only  in Mem[p_dst_first..p_dst_first+*p_dst_len-1].       */
UBYTE *p_wrk_mem;
UBYTE *p_src_first;
ULONG  src_len;
UBYTE *p_dst_first;
ULONG *p_dst_len;
{
 /* Byte pointers p_src and p_dst scan through the input and output blocks.   */
 /* FOLLOWING TWO WERE REGISTER FOR C CODE. */
 UBYTE *p_src = p_src_first+FLAG_BYTES;
 UBYTE *p_dst = p_dst_first;

 /* The following variable is never modified and are used to control          */
 /* the main loop.                                                            */
 UBYTE *p_src_post = p_src_first+src_len;

 UBYTE **pointer;
 UBYTE *length;
 UBYTE *alphabet;
 UBYTE **p_pointer,**p_post_pointer;
 UBYTE *p_length;

 /* Check the leading copy flag to see if the compressor chose to use a copy  */
 /* operation instead of a compression operation. If a copy operation was     */
 /* used, then all we need to do is copy the data over, set the output length */
 /* and return.                                                               */
 if (*p_src_first==FLAG_COPY)
   {
    fast_copy(p_src_first+FLAG_BYTES,p_dst_first,src_len-FLAG_BYTES);
    *p_dst_len=src_len-FLAG_BYTES;
    return;
   }

 pointer  = (UBYTE **) ULONG_ALIGN_UP(p_wrk_mem);
 alphabet = (UBYTE *)  &pointer[MAX_NODES];
 length   = (UBYTE *)  &alphabet[ALPHABET_SIZE];

 {
  UCARD i;
  UBYTE **p_init_pointer  = pointer;
  UBYTE  *p_init_alphabet = alphabet;
  UBYTE  *p_init_length   = length;
  UBYTE length_init_value = 1;
  for (i=0;i<ALPHABET_SIZE;i++)
    {
     *p_init_pointer ++ = p_init_alphabet;
     *p_init_alphabet++ = i;
     *p_init_length  ++ = length_init_value;
    }
 }

 p_pointer = p_post_pointer = &pointer[MAX_NODES];

 while (p_src!=p_src_post)
   {
    ULONG items_to_go   = (p_src_post-p_src)>>1;
    ULONG entries_to_go = p_post_pointer-p_pointer;
    ULONG unroll;  /* WAS REGISTER FOR C CODE. */

    if (entries_to_go==0)
       {
        p_pointer=&pointer[ALPHABET_SIZE];
        p_length =&length [ALPHABET_SIZE];
        entries_to_go=p_post_pointer-p_pointer;
       }

    unroll= (items_to_go<entries_to_go) ? items_to_go : entries_to_go;

    ass(unroll>0,"unroll==0.");

    while (unroll--)
      {
       register UBYTE *p;
       register UCARD len;
       register UWORD phrase;

       phrase =*p_src++<<8;
       phrase|=*p_src++;
       p=pointer[phrase];
       len=length[phrase];
       *p_pointer++=p_dst;
       *p_length++=len+1;
       while (len--)
         {*p_dst++=*p++;}
      }

   } /* End outer while loop. */

 /* Write the length of the decompressed data before returning. */
 *p_dst_len=p_dst-p_dst_first;

//  printf("EXITING DECOMPRESSOR.\n");
}

/******************************************************************************/
/*                                End of LZRW5.C                              */
/******************************************************************************/
