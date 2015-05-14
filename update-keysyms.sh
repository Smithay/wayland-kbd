#!/bin/bash

X11_INCLUDEDIR="/usr/include/X11"
KEYSYMDEFS="${X11_INCLUDEDIR}/keysymdef.h
            ${X11_INCLUDEDIR}/XF86keysym.h
            ${X11_INCLUDEDIR}/Sunkeysym.h
            ${X11_INCLUDEDIR}/DECkeysym.h
            ${X11_INCLUDEDIR}/HPkeysym.h"

echo "//" > src/ffi/keysyms.rs
echo "// This file was auto-generated using the update-keysyms.sh script." >> src/ffi/keysyms.rs
echo "//" >> src/ffi/keysyms.rs
echo "" >> src/ffi/keysyms.rs
echo "#![allow(non_upper_case_globals)]" >> src/ffi/keysyms.rs
echo "" >> src/ffi/keysyms.rs

cat $KEYSYMDEFS | sed -e '/XK_Ydiaeresis\s*0x100000ee/d' \
                      -e '/#define _/d' \
                      -e 's/#define\s*\(\w*\)XK_/#define XKB_KEY_\1/' \
                      -e '/\(#ifdef\|#ifndef\|#endif\)/d' \
                      -e 's/#define/pub const/g' \
                      -e 's/0x\([0-9a-fA-F]*\)/:u32 = 0x\1;/g' >> src/ffi/keysyms.rs
