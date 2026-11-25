use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::{Picker as NativePicker, ProtocolType};
use ratatui_image::protocol::{Protocol, halfblocks::Halfblocks, iterm2::Iterm2, kitty::Kitty, sixel::Sixel};
use ratatui_image::{FontSize, Resize};

#[derive(Clone, Debug)]
pub(crate) struct Picker {
    font_size: FontSize,
    protocol: ProtocolType,
    tmux: bool,
}

impl Picker {
    pub(crate) fn new(font_size: FontSize, protocol: ProtocolType) -> Self {
        Self {
            font_size,
            protocol,
            tmux: NativePicker::halfblocks().tmux_detected(),
        }
    }

    pub(super) fn font_size(&self) -> FontSize {
        self.font_size
    }

    pub(super) fn protocol_type(&self) -> ProtocolType {
        self.protocol
    }

    pub(super) fn new_protocol(
        &self,
        image: DynamicImage,
        available: Size,
        resize: &Resize,
    ) -> Result<Protocol, ratatui_image::errors::Errors> {
        let natural = Resize::natural_size(&image, self.font_size);
        let width = u32::from(natural.width) * u32::from(self.font_size.width);
        let height = u32::from(natural.height) * u32::from(self.font_size.height);
        let fits = natural.width <= available.width && natural.height <= available.height;
        let aligned = image.width() == width || image.height() == height;

        if !matches!(resize, Resize::Scale(_)) && fits && aligned {
            return encode(image, natural, self);
        }

        let size = resize.size_for(&image, self.font_size, available);
        let image = resize.resize(&image, self.font_size, size, None);

        encode(image, size, self)
    }
}

#[cfg(test)]
#[path = "graphics_picker_tests.rs"]
mod tests;

fn encode(image: DynamicImage, size: Size, widget_state: &Picker) -> Result<Protocol, ratatui_image::errors::Errors> {
    match widget_state.protocol {
        ProtocolType::Halfblocks => Halfblocks::new(image, size).map(Protocol::Halfblocks),
        ProtocolType::Iterm2 => Iterm2::new(image, size, widget_state.tmux).map(Protocol::ITerm2),
        ProtocolType::Sixel => Sixel::new(image, size, widget_state.tmux).map(Protocol::Sixel),
        ProtocolType::Kitty => Kitty::new(image, size, super::next_id(), widget_state.tmux, false).map(Protocol::Kitty),
    }
}
