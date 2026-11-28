// Whether the site's tree is folded on a wide screen, as the reader last
// left it in this browser. The choice is kept in localStorage under
// `treeKey` and mirrored on <html> as data-tree="open" or "folded", which
// `treeScript` sets first thing in <body>, before anything below it paints. The tree's CSS reads
// the attribute, so a page never draws the tree and then hides it.

/** The localStorage key and the <html> attribute's name. */
export const treeKey = "link-tree";

/** Runs first in <body>, before the page below it paints: copies the kept choice onto <html>. */
export const treeScript = `try{var t=localStorage.getItem(${JSON.stringify(treeKey)});if(t==="open"||t==="folded")document.documentElement.dataset.tree=t}catch(e){}`;
