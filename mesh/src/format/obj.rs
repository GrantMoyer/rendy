//! Loading mesh data from obj format.

use log::trace;
use {
    crate::{mesh::MeshBuilder, Normal, Position, Tangent, TexCoord},
    smallvec::{smallvec, SmallVec},
    std::collections::{BTreeSet, HashMap},
    wavefront_obj::obj,
};

/// Load mesh data from obj.
pub fn load_from_obj(
    bytes: &[u8],
) -> Result<Vec<(MeshBuilder<'static>, Option<String>)>, failure::Error> {
    let string = std::str::from_utf8(bytes)?;
    let set = obj::parse(string).map_err(|e| {
        failure::format_err!(
            "Error during parsing obj-file at line '{}': {}",
            e.line_number,
            e.message
        )
    })?;
    load_from_data(set)
}

fn load_from_data(
    obj_set: obj::ObjSet,
) -> Result<Vec<(MeshBuilder<'static>, Option<String>)>, failure::Error> {
    // Takes a list of objects that contain geometries that contain shapes that contain
    // vertex/texture/normal indices into the main list of vertices, and converts to
    // MeshBuilders with Position, Normal, TexCoord.
    trace!("Loading mesh");
    let mut objects = vec![];

    for object in obj_set.objects {
        for geometry in &object.geometry {
            let mut builder = MeshBuilder::new();

            // Since vertices, normals, tangents, and texture coordinates share
            // indices in rendy, we need an index for each unique VTNIndex.
            // E.x. f 1/1/1, 2/2/1, and 1/2/1 needs three different vertices, even
            // though only two vertices are referenced in the soure wavefron OBJ.
            let indices = geometry
                .shapes
                .iter()
                .flat_map(|shape| {
                    let tri: Option<SmallVec<[_; 3]>> = match shape.primitive {
                        obj::Primitive::Triangle(i1, i2, i3) => Some(smallvec![i1, i2, i3]),
                        _ => None,
                    };
                    tri
                })
                .flatten()
                .collect::<BTreeSet<_>>();

            let positions = indices
                .iter()
                .map(|i| {
                    let obj::Vertex { x, y, z } = object.vertices[i.0];
                    Position([x as f32, y as f32, z as f32])
                })
                .collect::<Vec<_>>();

            let normals = indices
                .iter()
                .map(|i| {
                    if let Some(j) = i.2 {
                        let obj::Normal { x, y, z } = object.normals[j];
                        Normal([x as f32, y as f32, z as f32])
                    } else {
                        Normal([0.0, 0.0, 0.0])
                    }
                })
                .collect::<Vec<_>>();

            let tex_coords = indices
                .iter()
                .map(|i| {
                    if let Some(j) = i.1 {
                        let obj::TVertex { u, v, .. } = object.tex_vertices[j];
                        TexCoord([u as f32, v as f32])
                    } else {
                        TexCoord([0.0, 0.0])
                    }
                })
                .collect::<Vec<_>>();

            let index_map = indices
                .iter()
                .enumerate()
                .map(|(v, k)| (k, v as u32))
                .collect::<HashMap<_, _>>();

            let reindex = geometry
                .shapes
                .iter()
                .flat_map(|shape| {
                    let tri: Option<SmallVec<[_; 3]>> = match shape.primitive {
                        obj::Primitive::Triangle(i1, i2, i3) => {
                            Some(smallvec![index_map[&i1], index_map[&i2], index_map[&i3],])
                        }
                        _ => None,
                    };
                    tri
                })
                .flatten()
                .collect::<Vec<_>>();

            //let tangents = Vec::new();

            debug_assert!(&normals.len() == &positions.len());
            //debug_assert!(&tangents.len() == &positions.len());
            debug_assert!(&tex_coords.len() == &positions.len());

            builder.add_vertices(positions);
            builder.add_vertices(normals);
            //builder.add_vertices(tangents);
            builder.add_vertices(tex_coords);
            builder.set_indices(reindex);

            // TODO: Add Material loading
            objects.push((builder, geometry.material_name.clone()))
        }
    }
    trace!("Loaded mesh");
    Ok(objects)
}

// compute tangent for the first vertex of a tri from vertex positions
// and texture coordinates
fn compute_tangent(tri: &[(&Position, &TexCoord)]) -> Tangent {
    let (a_obj, b_obj, c_obj) = (&(tri[0].0).0, &(tri[1].0).0, &(tri[2].0).0);
    let (a_tex, b_tex, c_tex) = (&(tri[0].1).0, &(tri[1].1).0, &(tri[2].1).0);

    let tspace_1_1 = b_tex[0] - a_tex[0];
    let tspace_2_1 = b_tex[1] - a_tex[1];

    let tspace_1_2 = c_tex[0] - a_tex[0];
    let tspace_2_2 = c_tex[1] - a_tex[1];

    let ospace_1_1 = b_obj[0] - a_obj[0];
    let ospace_2_1 = b_obj[1] - a_obj[1];
    let ospace_3_1 = b_obj[2] - a_obj[2];

    let ospace_1_2 = c_obj[0] - a_obj[0];
    let ospace_2_2 = c_obj[1] - a_obj[1];
    let ospace_3_2 = c_obj[2] - a_obj[2];

    let tspace_det = tspace_1_1 * tspace_2_2 - tspace_1_2 * tspace_2_1;

    let tspace_inv_1_1 = tspace_2_2 / tspace_det;
    let tspace_inv_2_1 = -tspace_2_1 / tspace_det;
    Tangent([
        ospace_1_1 * tspace_inv_1_1 + ospace_1_2 * tspace_inv_2_1,
        ospace_2_1 * tspace_inv_1_1 + ospace_2_2 * tspace_inv_2_1,
        ospace_3_1 * tspace_inv_1_1 + ospace_3_2 * tspace_inv_2_1,
        1.0,
    ])
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_load_from_obj() {
        let quad = b"v -1.000000 -1.000000 1.000000\nv 1.000000 -1.000000 1.000000\nv -1.000000 1.000000 1.000000\nv 1.000000 1.000000 1.000000\nv -1.000000 1.000000 -1.000000\nv 1.000000 1.000000 -1.000000\nv -1.000000 -1.000000 -1.000000\nv 1.000000 -1.000000 -1.000000\n
vt 0.000000 0.000000\nvt 1.000000 0.000000\nvt 0.000000 1.000000\nvt 1.000000 1.000000\n
vn 0.000000 0.000000 1.000000\nvn 0.000000 1.000000 0.000000\nvn 0.000000 0.000000 -1.000000\nvn 0.000000 -1.000000 0.000000\nvn 1.000000 0.000000 0.000000\nvn -1.000000 0.000000 0.000000\n
s 1
f 1/1/1 2/2/1 3/3/1\nf 3/3/1 2/2/1 4/4/1
s 2
f 3/1/2 4/2/2 5/3/2\nf 5/3/2 4/2/2 6/4/2
s 3
f 5/4/3 6/3/3 7/2/3\nf 7/2/3 6/3/3 8/1/3
s 4
f 7/1/4 8/2/4 1/3/4\nf 1/3/4 8/2/4 2/4/4
s 5
f 2/1/5 8/2/5 4/3/5\nf 4/3/5 8/2/5 6/4/5
s 6
f 7/1/6 1/2/6 5/3/6\nf 5/3/6 1/2/6 3/4/6
";
        let result = load_from_obj(quad).ok().unwrap();
        // dbg!(& result);
        assert_eq!(result.len(), 1);

        // When compressed into unique vertices there should be 4 vertices per side of the quad
        // assert!()
    }
}
