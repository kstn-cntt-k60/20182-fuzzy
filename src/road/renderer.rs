mod chosen_path;
mod streetlight;

use streetlight::StreetLight;
use crate::config::Config;

use glium::implement_vertex;
use glium::uniform;
use glium::{
    Program, Display, 
    Surface,
};

use nalgebra as na;

use crate::bezier;
use crate::road;

use road::LocationId;

#[derive(Copy, Clone)]
pub struct Vertex {
    position: [f32; 2],
}

implement_vertex!(Vertex, position);

struct ChosenLaneCrossSection {
    lanes: Vec<(LocationId, LocationId)>,
    cross_sections: Vec<(LocationId, LocationId, LocationId)>,
}

struct LaneIndex {
    from: LocationId,
    to: LocationId,
    right_border_indices: Vec<u16>,
}

struct CrossSectionIndex {
    from: LocationId,
    across: LocationId,
    to: LocationId,
    right_border_indices: Vec<u16>,
}

type VertexBuffer = glium::VertexBuffer<Vertex>;
type IndexBuffer = glium::IndexBuffer<u16>;

pub struct RoadRenderer {
    lane_indices: Vec<LaneIndex>,
    cross_section_indices: Vec<CrossSectionIndex>,

    vertex_buffer: VertexBuffer,
    index_buffer: IndexBuffer,
    border_index_buffer: IndexBuffer,
    chosen_index_buffer: IndexBuffer,

    program: Program,
    pub road_color: [f32; 3],
    pub border_color: [f32; 3],
    pub chosen_color: [f32; 3],

    streetlight: StreetLight,
}

const BEZIER_VCOUNT: u32 = 16;

const VERTEX_SHADER_SRC: &'static str = r#"
    #version 140

    in vec2 position;

    uniform mat4 matrix;

    void main() {
        gl_Position = matrix * vec4(position, 0.0, 1.0);
    }
"#;

const FRAGMENT_SHADER_SRC: &'static str = r#"
    #version 140
    out vec4 color;

    uniform vec3 input_color;

    void main() {
        color = vec4(input_color, 1.0);
    }
"#;

fn add_vertex(vertices: &mut Vec<Vertex>, p: bezier::Point) -> u16 {
    let index = vertices.len();
    let bezier::Point { x, y } = p;
    vertices.push(Vertex { position: [x, y] });
    index as u16
}

fn update_from_beziers(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    border_indices: &mut Vec<u16>,
    
    road: &road::Road,
    left: &Vec<road::DirectedBezier>,
    right: &Vec<road::DirectedBezier>,
    right_indices: &mut Vec<u16>)
{
    let bezier_count = left.len();

    assert!(bezier_count > 0, "Len must not be zero");
    assert!(
        bezier_count == right.len(), 
        "Left and Right must be the same number of Beziers");

    let b1 = road.get_bezier(left[0]);
    let b2 = road.get_bezier(right[0]);

    let mut index1_prev = add_vertex(vertices, b1.pos(0.0));
    let mut index2_prev = add_vertex(vertices, b2.pos(0.0));

    border_indices.extend_from_slice(
        &[index1_prev, index2_prev]);

    for i in 0..bezier_count {
        let b1 = road.get_bezier(left[i]);
        let b2 = road.get_bezier(right[i]);
            
        for k in 0..BEZIER_VCOUNT {
            let v: f32 = (k + 1) as f32 / BEZIER_VCOUNT as f32;

            let a = b1.pos(v);
            let b = b2.pos(v);

            let i1 = add_vertex(vertices, a);
            let i2 = add_vertex(vertices, b);

            indices.extend_from_slice(
                &[index1_prev, index2_prev, i1, i1, index2_prev, i2]);

            border_indices.extend_from_slice(
                &[index1_prev, i1, index2_prev, i2]);

            right_indices.extend_from_slice(
                &[index2_prev, i2]);

            index1_prev = i1;
            index2_prev = i2;
        }
    }

    border_indices.extend_from_slice(
        &[index1_prev, index2_prev]);
}

fn construct_buffers(
    lane_indices: &mut Vec<LaneIndex>, 
    cross_section_indices: &mut Vec<CrossSectionIndex>,
    display: &Display, road: &road::Road) 
    -> (VertexBuffer, IndexBuffer, IndexBuffer)
{
    let mut vertices: Vec<Vertex> = vec![];
    let mut indices: Vec<u16> = vec![];
    let mut border_indices: Vec<u16> = vec![];

    for lane in &road.lanes {
        let mut lane_index = LaneIndex {
            from: lane.from, 
            to: lane.to,
            right_border_indices: Vec::new(),
        };

        update_from_beziers(
            &mut vertices,
            &mut indices, 
            &mut border_indices, 
            road, &lane.left, &lane.right, 
            &mut lane_index.right_border_indices);

        lane_indices.push(lane_index);
    }

    for cross_section in &road.cross_sections {
        let mut cross_section_index = CrossSectionIndex {
            from: cross_section.from,
            across: cross_section.across,
            to: cross_section.to,
            right_border_indices: Vec::new(),
        };

        update_from_beziers(
            &mut vertices,
            &mut indices, 
            &mut border_indices, 
            road, &cross_section.left, &cross_section.right,
            &mut cross_section_index.right_border_indices);

        cross_section_indices.push(cross_section_index);
    }

    let vertex_buffer = VertexBuffer::new(
        display, 
        &vertices
    ).unwrap();

    let index_buffer = IndexBuffer::new(
        display,
        glium::index::PrimitiveType::TrianglesList,
        &indices
    ).unwrap();

    let border_index_buffer = IndexBuffer::new(
        display,
        glium::index::PrimitiveType::LinesList,
        &border_indices
    ).unwrap();

    (vertex_buffer, index_buffer, border_index_buffer)
}

impl RoadRenderer {
    pub fn from(display: &Display, road: &road::Road, config: &Config) -> Self {
        let program = glium::Program::from_source(
            display, VERTEX_SHADER_SRC, FRAGMENT_SHADER_SRC, None).unwrap();

        let mut lane_indices: Vec<LaneIndex> = vec![];
        let mut cross_section_indices: Vec<CrossSectionIndex> = vec![];

        let (vertex_buffer, index_buffer, border_index_buffer) 
            = construct_buffers(
                &mut lane_indices, 
                &mut cross_section_indices,
                display, road
            );

        Self {
            lane_indices: lane_indices,
            cross_section_indices: cross_section_indices,

            vertex_buffer: vertex_buffer,
            index_buffer: index_buffer,
            border_index_buffer: border_index_buffer,

            chosen_index_buffer: IndexBuffer::empty(
                display,
                glium::index::PrimitiveType::LinesList,
                0
            ).unwrap(),

            program: program,
            road_color: [40.0/255.0, 40.0/255.0, 40.0/255.0],
            border_color: [0.0, 1.0, 1.0],
            chosen_color: [1.0, 0.0, 0.0],

            streetlight: StreetLight::new(display, road, config),
        }
    }

    pub fn render<T>(
        &self, target: &mut T, road: &road::Road,
        view_proj: &na::Matrix4<f32>)
        where T: Surface
    {
        use glium::draw_parameters::DrawParameters;
        let mut params: DrawParameters = Default::default();
        params.line_width = Some(1.0);

        let matrix_ref: &[[f32; 4]; 4] = view_proj.as_ref();

        let uniform = uniform! {
            matrix: *matrix_ref,
            input_color: self.road_color,
        };
        target.draw(
            &self.vertex_buffer,
            &self.index_buffer,
            &self.program,
            &uniform, 
            &params).unwrap();

        let uniform = uniform! {
            matrix: *matrix_ref,
            input_color: self.border_color,
        };
        target.draw(
            &self.vertex_buffer,
            &self.border_index_buffer,
            &self.program,
            &uniform, 
            &params).unwrap();

        let uniform = uniform! {
            matrix: *matrix_ref,
            input_color: self.chosen_color,
        };
        params.line_width = Some(3.0);
        target.draw(
            &self.vertex_buffer,
            &self.chosen_index_buffer,
            &self.program,
            &uniform, 
            &params).unwrap();

        self.streetlight.render(target, road, view_proj);
    }

    pub fn update(&mut self, display: &Display, road: &road::Road) {
        if road.chosen_path_changed() {
            self.update_chosen_path(display, &road.chosen_path);
        }
    }
}
