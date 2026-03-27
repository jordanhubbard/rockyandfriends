"""
Natasha's OmniGraph RTX render script — uses GpuInteropCpuToDisk pipeline.
Based on test_og_rtx_save_to_disk.py from Kit 110 scripts.

Launch via omni.app.full.sh:
  DISPLAY=:1 ./omni.app.full.sh \
    --exec /home/jkh/.openclaw/workspace/renders/natasha_og_render.py \
    --/app/window/width=1920 \
    --/app/window/height=1080 \
    --/rtx/rendermode=PathTracing \
    --no-window
"""
import asyncio
import carb
import carb.eventdispatcher
import omni.kit.app
import omni.usd
from omni.kit.viewport.utility import get_active_viewport
from pxr import Sdf, Usd, UsdRender
# omni.graph.core is imported lazily inside create_render_product()
# because the extension loads after module-level --exec evaluation

USD_PATH = "/home/jkh/.openclaw/workspace/renders/horde_factory_floor.usda"
SAVE_FOLDER = "/home/jkh/.openclaw/workspace/renders/og_output"
AOVS = ["LdrColor"]
FRAME_START = 60    # start writing after 60 frames (RTX path convergence)
FRAME_COUNT = 1     # capture 1 frame
INFLIGHT_IO = 2

_frame_no = 0
_event_sub = None
_shutdown_requested = False


def load_callback(result, err):
    if result:
        carb.log_info("[natasha_og] Stage loaded OK")
        settings = carb.settings.get_settings()
        # Force resolution
        settings.set_int("/app/window/width", 1920)
        settings.set_int("/app/window/height", 1080)
        vp = get_active_viewport()
        if vp:
            vp.resolution = (1920, 1080)
            vp.camera_path = "/World/RenderCamera"
            carb.log_info("[natasha_og] Camera → /World/RenderCamera, res 1920×1080")
        asyncio.ensure_future(create_render_product())
    else:
        carb.log_error(f"[natasha_og] Stage load failed: {err}")
        omni.kit.app.get_app().post_quit(-1)


async def do_shutdown():
    global _event_sub
    carb.log_info("[natasha_og] Shutting down.")
    omni.usd.get_context().close_stage(None)
    _event_sub = None
    omni.kit.app.get_app().post_quit(0)


def on_rendering_event(e: carb.eventdispatcher.Event):
    global _frame_no, _shutdown_requested
    _frame_no = e["frame_number"]
    if not _shutdown_requested and _frame_no > (FRAME_START + FRAME_COUNT + 10):
        _shutdown_requested = True
        carb.log_info(f"[natasha_og] Frame {_frame_no} — capture window passed, shutting down.")
        asyncio.ensure_future(do_shutdown())


async def create_render_product():
    global _event_sub
    import os
    import omni.graph.core as og  # deferred import — extension must be fully loaded first
    os.makedirs(SAVE_FOLDER, exist_ok=True)

    try:
        viewport = get_active_viewport()
        usd_context = viewport.usd_context
        stage = usd_context.get_stage()
        session_layer = stage.GetSessionLayer()

        await usd_context.next_frame_async(viewport)

        # Duplicate the existing hydra render product so we get a clean OmniGraph pipeline
        render_prod_src = "/Render/OmniverseKit/HydraTextures/omni_kit_widget_viewport_ViewportTexture_0"
        render_prod_dupe = "/Render/RenderProduct_Natasha"
        pipeline_path = f"{render_prod_dupe}/Pipeline"

        with Usd.EditContext(stage, session_layer):
            omni.kit.commands.execute("CopyPrim", path_from=render_prod_src, path_to=render_prod_dupe)

            rp_dup_prim = stage.GetPrimAtPath(render_prod_dupe)
            rp_dup_prim.CreateAttribute("ogPostProcessPath", Sdf.ValueTypeNames.String).Set(pipeline_path)

            # Define render vars (AOVs)
            ordered_var_paths = []
            for aov in AOVS:
                new_var = UsdRender.Var.Define(stage, f"/Render/Vars/{aov}")
                new_var.GetSourceNameAttr().Set(aov)
                ordered_var_paths.append(new_var.GetPrim().GetPath())
            UsdRender.Product(rp_dup_prim).GetOrderedVarsRel().SetTargets(ordered_var_paths)

            # Build the OmniGraph post-render pipeline
            orchestration_graphs = og.get_global_orchestration_graphs_in_pipeline_stage(
                og.GraphPipelineStage.GRAPH_PIPELINE_STAGE_POSTRENDER)
            orchestration_graph = orchestration_graphs[0]

            (result, wrapper_node) = og.cmds.CreateGraphAsNode(
                graph=orchestration_graph,
                node_name="Pipeline",
                graph_path=pipeline_path,
                evaluator_name="push",
                is_global_graph=True,
                backed_by_usd=True,
                fc_backing_type=og.GraphBackingType.GRAPH_BACKING_TYPE_FABRIC_SHARED,
                pipeline_stage=og.GraphPipelineStage.GRAPH_PIPELINE_STAGE_POSTRENDER)

            wrapped_graph = wrapper_node.get_wrapped_graph()

            # Entry node
            og.cmds.CreateNode(graph=wrapped_graph,
                               node_path=f"{pipeline_path}/RenderProductEntry",
                               node_type="omni.graph.nodes.GpuInteropRenderProductEntry",
                               create_usd=True)
            entry_node = wrapped_graph.get_node(f"{pipeline_path}/RenderProductEntry")
            entry_rp_attr = entry_node.get_attribute("outputs:rp")
            entry_gpu_attr = entry_node.get_attribute("outputs:gpu")

            for aov in AOVS:
                safe = aov.replace(":", "_")

                # GPU→CPU copy node
                g2c_path = f"{pipeline_path}/GpuToCpu_{safe}"
                og.cmds.CreateNode(graph=wrapped_graph,
                                   node_path=g2c_path,
                                   node_type="omni.graph.examples.cpp.GpuInteropGpuToCpuCopy",
                                   create_usd=True)
                g2c_node = wrapped_graph.get_node(g2c_path)
                og.cmds.ConnectAttrs(src_attr=entry_rp_attr, dest_attr=g2c_node.get_attribute("inputs:rp"), modify_usd=True)
                og.cmds.ConnectAttrs(src_attr=entry_gpu_attr, dest_attr=g2c_node.get_attribute("inputs:gpu"), modify_usd=True)
                og.Controller.set(g2c_node.get_attribute("inputs:aovGpu"), str(aov))

                # CPU→Disk write node
                c2d_path = f"{pipeline_path}/CpuToDisk_{safe}"
                og.cmds.CreateNode(graph=wrapped_graph,
                                   node_path=c2d_path,
                                   node_type="omni.graph.examples.cpp.GpuInteropCpuToDisk",
                                   create_usd=True)
                c2d_node = wrapped_graph.get_node(c2d_path)
                og.cmds.ConnectAttrs(src_attr=g2c_node.get_attribute("outputs:rp"), dest_attr=c2d_node.get_attribute("inputs:rp"), modify_usd=True)
                og.cmds.ConnectAttrs(src_attr=g2c_node.get_attribute("outputs:gpu"), dest_attr=c2d_node.get_attribute("inputs:gpu"), modify_usd=True)
                og.cmds.ConnectAttrs(src_attr=g2c_node.get_attribute("outputs:aovCpu"), dest_attr=c2d_node.get_attribute("inputs:aovCpu"), modify_usd=True)
                og.Controller.set(c2d_node.get_attribute("inputs:aovGpu"), str(aov))
                og.Controller.set(c2d_node.get_attribute("inputs:startFrame"), FRAME_START)
                og.Controller.set(c2d_node.get_attribute("inputs:frameCount"), FRAME_COUNT)
                og.Controller.set(c2d_node.get_attribute("inputs:saveLocation"), SAVE_FOLDER)
                og.Controller.set(c2d_node.get_attribute("inputs:fileType"), "png")
                og.Controller.set(c2d_node.get_attribute("inputs:maxInflightWrites"), INFLIGHT_IO)

        # Switch viewport to the new render product
        viewport.render_product_path = render_prod_dupe
        carb.log_info(f"[natasha_og] OmniGraph pipeline ready. Will capture frame {FRAME_START}–{FRAME_START+FRAME_COUNT} → {SAVE_FOLDER}")

        # Subscribe to new-frame events to trigger shutdown after capture
        usd_ctx = omni.usd.get_context()
        _event_sub = carb.eventdispatcher.get_eventdispatcher().observe_event(
            event_name=usd_ctx.stage_rendering_event_name(omni.usd.StageRenderingEventType.NEW_FRAME, True),
            on_event=on_rendering_event,
            observer_name="natasha_og_render")

    except Exception as exc:
        carb.log_error(f"[natasha_og] Pipeline setup failed: {exc}")
        import traceback
        carb.log_error(traceback.format_exc())
        omni.kit.app.get_app().post_quit(-1)


def main():
    carb.log_info(f"[natasha_og] Loading USD: {USD_PATH}")
    omni.usd.get_context().open_stage_with_callback(USD_PATH, load_callback)


if __name__ == "__main__":
    main()
