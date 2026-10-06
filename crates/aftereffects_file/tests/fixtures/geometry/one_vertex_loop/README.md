# Native one-vertex closed cubic loop

Independently authored in AE26.5x89 through managed `Client.run_jsx` request
`w06-one-vertex-loop-author-v1`. `author.jsx` is the exact ES3 managed function
body, not a standalone runner. Native SHA256:
`9908039bd36629cf8121841cbaea381558bc0c4b5d445395a394d53f1e89f753`.
Comp1 `W06 One Vertex Bezier`,320×240,square pixels,24fps,full2s.

Source save/reopen independently observed one closed vertex[0,0], incoming
[90,-100], outgoing[-90,-100]. These nonzero opposing handles define a drawable
closed cubic even though the endpoint returns to the same sole vertex. Layer
position[160,160], cyan fill and white4px stroke make a nonblank control.

The regression freshly imports these native bytes and checks editable
MoveTo/CubicTo/Close geometry and handles. Fresh export formerly omitted the
Path; after repair it retains the folded one-vertex native contour. Input editing
translates each imported Path40px on X, including its authoritative constant
animator values, before another fresh export. No source bytes/inputs are pruned,
no native donor is replayed, and no renderer/schema changes are used.

Managed readback request `w06-one-vertex-loop-readback-v1` opens the source and
the independently generated unedited/edited AEPs. `readback.jsx` verifies every
native vector Path:closed,one vertex,original relative tangents,vertices[0,0]
or[40,0] as appropriate. Generated three paths arise from existing editable
paint/geometry splitting; count equality with the native source is not claimed.
The managed operation passed and returned verified READY.

`evidence.json` pins controls, source/export hashes and converter receipt.
Independent source render at Adobe30fps (`w06-one-vertex-loop-reference-v1`)
preserved24fps source bytes/full2s and produced60 frames. All decoded frames
are nonblank (3230–3234 nonblack pixels); this is a sanity check, **not** proof
of the curve's appearance or converter fidelity. No model inspection/scoring.
Video is ignored/local, not Git. Asset publication/fresh download and strict
RGB/alpha comparisons remain incomplete. Audio is not exercised.

Import implementation is unchanged; concrete native-source editable import
assertions passed. Export has native open/control/input-edit proof for this
bounded static control, not arbitrary animation/topology/render fidelity.
This source is a new public minimal control, not an original49 repair claim.
See the support ledger for all limitations.
