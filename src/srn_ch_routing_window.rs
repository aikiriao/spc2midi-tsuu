use crate::types::*;
use crate::Message;
use iced::widget::{button, checkbox, column, pick_list, row, text, tooltip, Column};
use iced::{alignment, Element, Length};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct SRNChannelRoutingWindow {
    title: String,
    srn_no: u8,
    source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
}

impl SPC2MIDI2Window for SRNChannelRoutingWindow {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<'_, Message> {
        let params = self.source_parameter.read().unwrap();
        let param = params.get(&self.srn_no).unwrap();
        // ドラム音色が選択されているときはチャンネル候補を絞る
        let output_midi_channel_list = if (param.program.clone() as u8) >= 0x80 {
            vec![9]
        } else {
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15]
        };
        let mut status_list: Vec<_> = (0..8)
            .map(|ch| {
                row![
                    checkbox(param.channel_mute[ch])
                        .on_toggle(move |flag| Message::ChannelRoutingMuteChanged(
                            self.srn_no,
                            ch as u8,
                            flag
                        ))
                        .width(20),
                    text(format!("{}", ch))
                        .align_x(alignment::Alignment::Center)
                        .align_y(alignment::Alignment::Center)
                        .height(Length::Fill)
                        .width(Length::FillPortion(1)),
                    text("→").width(10),
                    pick_list(
                        output_midi_channel_list.clone(),
                        Some(param.channel_routing[ch]),
                        move |dst_ch| {
                            Message::ChannelRoutingChanged(self.srn_no, ch as u8, dst_ch)
                        }
                    )
                    .width(Length::FillPortion(1)),
                ]
                .spacing(10)
                .width(Length::Fill)
                .align_y(alignment::Alignment::Center)
                .into()
            })
            .collect();

        // インデックス
        let ch_index = row![
            text("Mute").width(20).align_x(alignment::Alignment::Start),
            text("SPC Channel")
                .width(Length::FillPortion(1))
                .align_x(alignment::Alignment::Center),
            text("").width(10),
            text("MIDI Channel")
                .width(Length::FillPortion(1))
                .align_x(alignment::Alignment::Center),
        ]
        .spacing(10)
        .width(Length::Fill)
        .align_y(alignment::Alignment::Center);

        status_list.insert(0, ch_index.into());

        // 操作ボタン
        let controller = row![tooltip(
            button("Reset").on_press(Message::ChannelRoutingReseted(self.srn_no)),
            "Reset Channel Routing",
            tooltip::Position::Top,
        ),];

        column![
            Column::from_vec(status_list).width(Length::Fill),
            controller,
        ]
        .into()
    }
}

impl SRNChannelRoutingWindow {
    pub fn new(
        title: String,
        srn_no: u8,
        source_parameter: Arc<RwLock<BTreeMap<u8, SourceParameter>>>,
    ) -> Self {
        Self {
            title: title,
            srn_no: srn_no,
            source_parameter: source_parameter,
        }
    }
}
