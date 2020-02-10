//! Loading mesh data from obj format.

use log::trace;
use {
    crate::{mesh::MeshBuilder, Normal, Position, Tangent, TexCoord},
    mikktspace::Geometry,
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
            // We also don't want triangle with opposite windings to share a vertex.
            let tris = geometry
                .shapes
                .iter()
                .flat_map(|shape| match shape.primitive {
                    obj::Primitive::Triangle(i1, i2, i3) => {
                        let h = handedness(
                            object.vertices[i1.0],
                            object.vertices[i2.0],
                            object.vertices[i3.0],
                        );
                        Some([(i1, h), (i2, h), (i3, h)])
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            let indices = tris.iter().flatten().collect::<BTreeSet<_>>();

            let positions = indices
                .iter()
                .map(|(i, _)| {
                    let obj::Vertex { x, y, z } = object.vertices[i.0];
                    Position([x as f32, y as f32, z as f32])
                })
                .collect::<Vec<_>>();

            let normals = indices
                .iter()
                .map(|(i, _)| {
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
                .map(|(i, _)| {
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

            let reindex = tris
                .iter()
                .flatten()
                .map(|i| index_map[&i])
                .collect::<Vec<_>>();

            let tangents = {
                let mut obj_geom = ObjGeometry::new(&positions, &normals, &tex_coords, &reindex);
                if !mikktspace::generate_tangents(&mut obj_geom) {
                    return Err(failure::format_err!("Geometry is unsuitable for tangent generation"));
                }
                obj_geom.get_tangents()
            };

            debug_assert!(&normals.len() == &positions.len());
            debug_assert!(&tangents.len() == &positions.len());
            debug_assert!(&tex_coords.len() == &positions.len());

            builder.add_vertices(positions);
            builder.add_vertices(normals);
            builder.add_vertices(tangents);
            builder.add_vertices(tex_coords);
            builder.set_indices(reindex);

            // TODO: Add Material loading
            objects.push((builder, geometry.material_name.clone()))
        }
    }
    trace!("Loaded mesh");
    Ok(objects)
}

fn handedness(a: obj::Vertex, b: obj::Vertex, c: obj::Vertex) -> i8 {
    let d = obj::Vertex {
        x: b.x - a.x,
        y: b.y - a.y,
        z: b.z - a.z,
    };
    let e = obj::Vertex {
        x: c.x - a.x,
        y: c.y - a.y,
        z: c.z - a.z,
    };
    let cross = obj::Vertex {
        x: d.y * e.z - d.z * e.y,
        y: d.z * e.x - d.x * e.z,
        z: d.x * e.y - d.y * e.x,
    };
    (cross.x * cross.x + cross.y * cross.y + cross.z * cross.z).signum() as i8
}

// Only supports tris, therefore indices.len() must be divisible by 3, and
// assumes each 3 vertices represents a tri
struct ObjGeometry<'a> {
    positions: &'a Vec<Position>,
    normals: &'a Vec<Normal>,
    tex_coords: &'a Vec<TexCoord>,
    indices: &'a Vec<u32>,
    tangents: Vec<Tangent>,
}

impl<'a> ObjGeometry<'a> {
    fn new(
        positions: &'a Vec<Position>,
        normals: &'a Vec<Normal>,
        tex_coords: &'a Vec<TexCoord>,
        indices: &'a Vec<u32>,
    ) -> Self {
        Self {
            positions,
            normals,
            tex_coords,
            indices,
            tangents: vec![Tangent([0.0, 0.0, 0.0, 1.0]); positions.len()],
        }
    }

    fn accumulate_tangent(&mut self, index: usize, other: [f32; 4]) {
        let acc = &mut self.tangents[index];
        acc.0[0] += other[0];
        acc.0[1] += other[1];
        acc.0[2] += other[2];
        acc.0[3] = other[3];
    }

    fn normalize_tangent(Tangent([x, y, z, w]): &Tangent) -> Tangent {
        let len = x * x + y * y + z * z;
        Tangent([x / len, y / len, z / len, *w])
    }

    fn get_tangents(&self) -> Vec<Tangent> {
        self.tangents
            .iter()
            .map(Self::normalize_tangent)
            .collect::<Vec<_>>()
    }
}

impl Geometry for ObjGeometry<'_> {
    fn num_faces(&self) -> usize {
        self.indices.len() / 3
    }

    fn num_vertices_of_face(&self, _: usize) -> usize {
        3
    }

    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        self.positions[self.indices[face * 3 + vert] as usize].0
    }

    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        self.normals[self.indices[face * 3 + vert] as usize].0
    }

    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        self.tex_coords[self.indices[face * 3 + vert] as usize].0
    }

    fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
        // Not supposed to just average tangents over existing index,
        // since triangles could be welded using different asumptions than
        // Mikkelsen used. However, we *do* use basically the same assumptions,
        // except that some vertices Mikkelsen expects to be welded may not be
        // if they aren't in the source OBJ.
        self.accumulate_tangent(self.indices[face * 3 + vert] as usize, tangent);
    }
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
