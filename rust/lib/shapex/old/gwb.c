/*
**    Geometric WorkBench
**
**    Copyright (c) 1988
**    Martti Mantyla, Helsinki University of Technology
**
**    This software is NOT released to public domain, but a
**    permission is granted to inspect the code and use it, or
**    portions of it for research or educational purposes, provided
**    that this notice is not removed or modified.  Commercial
**    exploitation is disallowed.
**
**    THE SOFTWARE IS PROVIDED "AS IS" WITH NO WARRANTY OF ANY KIND.
**    THE AUTHOR SPECIFICALLY DISCLAIMS ANY EXPRESS OR IMPLIED
**    WARRANTY OF FITNESS FOR A PARTICULAR PURPOSE AND ANY WARRANTY
**    OF MERCHANTABILITY.
*/


Solid   *mvfs(fn, vn, x, y, z)
Id      fn, vn;
float   x, y, z;
{
  Solid           *newsolid;
  Face            *newface;
  Vertex          *newvertex;
  HalfEdge        *newhe;
  Loop            *newloop;

  newsolid = (Solid *) new(SOLID, NIL);
  newface = (Face *) new(FACE, newsolid);
  newloop = (Loop *) new(LOOP, newface);
  newvertex = (Vertex *) new(VERTE, newsolid);
  newhe = (HalfEdge *) new(HALFEDGE, NIL);

  newface->faceno = fn;
  newface->flout = newloop;

  newloop->ledg = newhe;

  newhe->wloop = newloop;
  newhe->nxt = newhe->prv = newhe;
  newhe->vtx = newvertex;
  newhe->edg = NIL;

  newvertex->vertexno = vn;
  newvertex->vcoord[0] = x;
  newvertex->vcoord[1] = y;
  newvertex->vcoord[2] = z;
  newvertex->vcoord[3] = 1.0;

  return(newsolid);
}


void            lmev(he1, he2, vn, x, y, z)
HalfEdge        *he1, *he2;
Id              vn;
float           x, y, z;
{
  HalfEdge        *he;
  Vertex          *newvertex;
  Edge            *newedge;

  newedge = (Edge *) new(EDGE, he1->wloop->lface->fsolid);
  newvertex = (Vertex *) new(VERTE, he1->wloop->lface->fsolid);
  newvertex->vcoord[0] = x;
  newvertex->vcoord[1] = y;
  newvertex->vcoord[2] = z;
  newvertex->vcoord[3] = 1.0;
  newvertex->vertexno = vn;

  he = he1;
  while(he != he2)
  {
    he->vtx = newvertex;
    he = mate(he)->nxt;
  }

  addhe(newedge, he2->vtx, he1, MINUS);
  addhe(newedge, newvertex, he2, PLUS);

  newvertex->vedge = he2->prv;
  he2->vtx->vedge = he2;
}


Face            *lmef(he1, he2, nf)
HalfEdge        *he1, *he2;
Id        nf;
{
  Face            *newface;
  Loop            *newloop;
  Edge            *newedge;
  HalfEdge        *he, *nhe1, *nhe2, *temp;

  newface = (Face *)new(FACE, he1->wloop->lface->fsolid);
  newloop = (Loop *)new(LOOP, newface);
  newedge = (Edge *)new(EDGE, he1->wloop->lface->fsolid);
  newface->faceno = nf;
  newface->flout = newloop;

  he = he1;
  while(he != he2)
  {
    he->wloop = newloop;
    he = he->nxt;
  }

  nhe1 = addhe(newedge, he2->vtx, he1, MINUS);
  nhe2 = addhe(newedge, he1->vtx, he2, PLUS);

  nhe1->prv->nxt = nhe2;
  nhe2->prv->nxt = nhe1;
  temp = nhe1->prv;
  nhe1->prv = nhe2->prv;
  nhe2->prv = temp;

  newloop->ledg = nhe1;
  he2->wloop->ledg = nhe2;

  return(newface);
}


HalfEdge        *addhe(e, v, where, sign)
Edge            *e;
Vertex          *v;
HalfEdge        *where;
int             sign;
{
  HalfEdge        *he;

  if(where->edg == NIL)
  {
    he = where;
  }
  else
  {
    he = (HalfEdge *) new(HALFEDGE, NIL);
    where->prv->nxt = he;
    he->prv = where->prv;
    where->prv = he;
    he->nxt = where;
  }

  he->edg = e;
  he->vtx = v;
  he->wloop = where->wloop;
  if(sign == PLUS)
          e->he1 = he;
  else    e->he2 = he;

  return(he);
}


void    sweep(fac, dx, dy, dz)
Face    *fac;
float   dx, dy, dz;
{
  Loop            *l;
  HalfEdge        *first, *scan;
  Vertex          *v;

  getmaxnames(fac->fsolid);
  l = fac->floops;
  while(l)
  {
    first = l->ledg;
    scan = first->nxt;
    v = scan->vtx;
    lmev(scan, scan, ++maxv,
        v->vcoord[0] + dx,
        v->vcoord[1] + dy,
        v->vcoord[2] + dz);
    while(scan != first)
    {
      v = scan->nxt->vtx;
      lmev(scan->nxt, scan->nxt, ++maxv,
              v->vcoord[0] + dx,
              v->vcoord[1] + dy,
              v->vcoord[2] + dz);
      lmef(scan->prv, scan->nxt->nxt, ++maxf);
      scan = mate(scan->nxt)->nxt;
    }
    lmef(scan->prv, scan->nxt->nxt, ++maxf);
    l = l->nextl;
  }
}


Solid   *block(dx, dy, dz)
float   dx, dy, dz;
{
  Solid        *s;

  s = mvfs(1, 1, 0.0, 0.0, 0.0);
  mev(s, 1, 1, 2, dx, 0.0, 0.0);
  mev(s, 1, 2, 3, dx, dy, 0.0);
  mev(s, 1, 3, 4, 0.0, dy, 0.0);
  mef(s, 1, 4, 1, 2);
  sweep(fface(s, 1), 0.0, 0.0, dz);
  return(s);
}


void            lkemr(h1, h2)
HalfEdge        *h1, *h2;
{
  HalfEdge    *h3, *h4;
  Loop            *nl;
  Loop            *ol;
  Edge            *killedge;

  ol = h1->wloop;
  nl = (Loop *) new(LOOP, (Node *)ol->lface);
  killedge = h1->edg;

  h3 = h1->nxt;
  h1->nxt = h2->nxt;
  h2->nxt->prv = h1;
  h2->nxt = h3;
  h3->prv = h2;

  h4 = h2;
  do
  {
    h4->wloop = nl;
  }
  while((h4 = h4->nxt) != h2);

  ol->ledg = h3 = delhe(h1);
  nl->ledg = h4 = delhe(h2);

  h3->vtx->vedge = (h3->edg) ? h3 : (HalfEdge *)NIL;
  h4->vtx->vedge = (h4->edg) ? h4 : (HalfEdge *)NIL;
  h3->nxt->vtx->vedge = (h3->nxt->edg) ? h3->nxt : (HalfEdge *)NIL;
  h4->nxt->vtx->vedge = (h4->nxt->edg) ? h4->nxt : (HalfEdge *)NIL;

  del(EDGE, (Node *)killedge, ol->lface->fsolid);
}


void    lkfmrh(f1, f2)
Face    *f1, *f2;
{
  Loop        *l;

  while(l = f2->floops)
  {
    dellist(LOOP, l, f2);
    addlist(LOOP, l, f1);
  }
  del(FACE, f2, f1->fsolid);
}


HalfEdge        *delhe(he)
HalfEdge        *he;
{
  if(he->edg == NIL)
  {
    del(HALFEDGE, he, NIL);
    return(NIL);
  }
  else if(he->nxt == he)
  {
    he->edg = NIL;
    return(he);
  }
  else
  {
    he->prv->nxt = he->nxt;
    he->nxt->prv = he->prv;
    del(HALFEDGE, he, NIL);
    return(he->prv);
  }
}


void    addlist(what, which, where)
int     what;
Node    *which, *where;
{
  switch(what)
  {
    case FACE:
      which->f.nextf = where->s.sfaces;
      which->f.prevf = (Face *) NIL;
      if(where->s.sfaces)
              where->s.sfaces->prevf = (Face *) which;
      where->s.sfaces = (Face *) which;
      which->f.fsolid = (Solid *) where;
      break;
    case LOOP:
      which->l.nextl = where->f.floops;
      which->l.prevl = (Loop *) NIL;
      if(where->f.floops)
              where->f.floops->prevl = (Loop *) which;
      where->f.floops = (Loop *) which;
      which->l.lface = (Face *) where;
      break;
    case EDGE:
      which->e.nexte = where->s.sedges;
      which->e.preve = (Edge *) NIL;
      if(where->s.sedges)
              where->s.sedges->preve = (Edge *) which;
      where->s.sedges = (Edge *) which;
      break;
    case VERTEX:
      which->v.nextv = where->s.sverts;
      which->v.prevv = (Vertex *) NIL;
      if(where->s.sverts)
              where->s.sverts->prevv = (Vertex *) which;
      where->s.sverts = (Vertex *) which;
      break;
  }
}


void    dellist(what, which, where)
int    what;
Node    *which, *where;
{
  switch(what)
  {
    case FACE:
      if(which->f.prevf)
          which->f.prevf->nextf = which->f.nextf;
      if(which->f.nextf)
          which->f.nextf->prevf = which->f.prevf;
      if((Face *) which == where->s.sfaces)
          where->s.sfaces = which->f.nextf;
      break;
    case LOOP:
      if(which->l.prevl)
          which->l.prevl->nextl = which->l.nextl;
      if(which->l.nextl)
          which->l.nextl->prevl = which->l.prevl;
      if((Loop *) which == where->f.floops)
          where->f.floops = which->l.nextl;
      break;
    case EDGE:
      if(which->e.preve)
          which->e.preve->nexte = which->e.nexte;
      if(which->e.nexte)
          which->e.nexte->preve = which->e.preve;
      if((Edge *) which == where->s.sedges)
          where->s.sedges = which->e.nexte;
      break;
    case VERTEX:
      if(which->v.prevv)
          which->v.prevv->nextv = which->v.nextv;
      if(which->v.nextv)
          which->v.nextv->prevv = which->v.prevv;
      if((Vertex *) which == where->s.sverts)
          where->s.sverts = which->v.nextv;
      break;
  }
}


void    del(what, which, where)
int     what;
Node    *which;
Node    *where;
{
  switch(what)
  {
    case SOLID:
        break;
    case FACE:
        dellist(FACE, which, where);
        break;
    case LOOP:
        dellist(LOOP, which, where);
        break;
    case EDGE:
        dellist(EDGE, which, where);
        break;
    case HALFEDGE:
        break;
    case VERTEX:
        dellist(VERTEX, which, where);
        break;
  }
  free(which);
}
