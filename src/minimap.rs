use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    },
};
use bevy_egui::{EguiTextureHandle, EguiUserTextures};

use crate::generate;

#[derive(Resource, Deref)]
pub struct MinimapImage(Handle<Image>);
#[derive(Component)]
pub struct MinimapControlledArea(pub Entity);
pub fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut egui_user_textures: ResMut<EguiUserTextures>,
) {
    let size = Extent3d {
        width: 1024,
        height: 576,
        ..default()
    };

    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("camera_minimap_texture"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size); // allocates backing data

    let handle = images.add(image);

    // Make it usable by egui:
    egui_user_textures.add_image(EguiTextureHandle::Strong(handle.clone()));

    commands.insert_resource(MinimapImage(handle));
}
pub fn spawn_minimap_camera(
    mut commands: Commands,
    minimap: Res<MinimapImage>,
    world_map: Res<generate::WorldMap>,
) {
    let world = world_map.0.as_ref().unwrap();
    let bounds = world.bounds();
    info!("World bounds: {:?}", bounds);
    commands.spawn((
        Camera2d,
        Camera {
            target: RenderTarget::Image(minimap.clone().into()),
            order: -1, // render before your main camera if you want
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scale: world.scale / 60.0,
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::from_layers(&[crate::render_layers::MINIMAP]), // render only what is on layer 1
        Transform::from_xyz(bounds.1.x * 0.5, bounds.1.y * 0.5, 0.0),
    ));

    // (Optional but recommended) isolate what this camera renders with RenderLayers.
}
